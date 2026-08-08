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

## 6. Rules with no source

Each item in this section states a rule that the decoder applies and that no
openNURBS writer produces. A record that the rule governs cannot decode from a
Rhino-authored file. Where `rhino_3dm.md` states the rule, the specification
carries the same defect and must change with the decoder.

### NS-01. Brep mesh-side wrapper version byte

**Question.** What is the first byte of a Brep mesh-side wrapper body?

**Known.** `ON_Brep::Write` in `opennurbs_brep_io.cpp` opens the wrapper with `file.BeginWrite3dmChunk( TCODE_ANONYMOUS_CHUNK, 0 )` and then writes one presence byte for each face with `file.WriteChar(b)`. No version field is between the chunk header and the first presence byte. `ON_Brep::Read` reads the same chunk with `BeginRead3dmBigChunk` and one `ReadChar` for each face. The legacy `ReadOld201` mesh path has the same shape.

**Conflict.** `read_mesh_sides` in `crates/cadmpeg-codec-rhino/src/brep.rs` reads a byte before the presence loop and refuses a nonzero value as an unsupported version. The first byte of the body is the presence byte of face 0. A face 0 that holds a render mesh gives 1 and the refusal fires. A face 0 that holds no render mesh gives 0, the byte passes as a version, and the presence loop is then short by one entry. Both paths reach the catch-all, which discards the wrapper and appends the free-text warning `Brep mesh cache degraded`. No Brep from a Rhino-authored file yields a render-mesh or analysis-mesh cache. `rhino_3dm.md` §19.4 "For Brep minor at least 1, each mesh-side wrapper is anonymous with packed" states the invented byte as fact.

**Need.** The wrapper body starts at the presence byte of face 0. The decoder and `rhino_3dm.md` §15 must both drop the version field. The failure must become a typed loss, because a free-text warning does not reach the loss census and hides the size of the gap.

### NS-02. Anonymous chunk minor version

**Question.** What minor version does `ON_BinaryArchive::BeginWrite3dmAnonymousChunk` write?

**Known.** `BeginWrite3dmAnonymousChunk(version)` in `opennurbs_archive.cpp` calls `BeginWrite3dmChunk(TCODE_ANONYMOUS_CHUNK, major_version, version)` with `major_version` fixed at 1. Its argument is the minor version. Six call sites in the openNURBS tree pass 1: `opennurbs_brep_io.cpp` for the Brep region wrapper, `opennurbs_font.cpp`, `opennurbs_object_history.cpp` for the SubD edge-chain value, and three in `opennurbs_subd.cpp`. `BeginRead3dmAnonymousChunk` accepts each minor version that is not less than zero and refuses only a major version other than 1.

**Conflict.** Three decode sites require minor version 0 where openNURBS writes 1.

- `read_regions` in `crates/cadmpeg-codec-rhino/src/brep.rs` refuses the Brep region wrapper unless the two leading `i32` fields are 1 and 0. `ON_Brep::Write` writes 1 and 1. The test never passes, `read_regions` returns empty vectors, and `validate_regions` is then skipped. Each solid at archive version 60 and above loses its region topology. The inner `ON_BrepRegionTopology` object is version 1.0 and the decoder reads it correctly, so only the outer wrapper is wrong.
- `subd_edge_chains` in `crates/cadmpeg-codec-rhino/src/history.rs` refuses the chain list, and the same function refuses each chain, unless the minor version is 0. `ON_SubDEdgeChainHistoryValue::WriteHelper` and `ON_SubDEdgeChain::Write` write 1, and both readers require a chunk version of at least 1. A history record that holds a SubD edge chain is discarded whole, with its command identity, antecedents, and descendants.

`rhino_3dm.md` §19.4 "For Brep minor 3, the region wrapper is anonymous version 1.0" states version 1.0 as fact. `rhino_3dm.md` §7.1 "A SubD edge-chain value is an anonymous version 1.0 chunk containing an `i32`" states the same for the SubD edge chain.

**Need.** Each anonymous chunk must carry the minor version that its openNURBS writer supplies. A decode site must accept a minor version that is not less than the one it needs, because `BeginRead3dmAnonymousChunk` applies that rule. The specification must change with the decoder.

### NS-03. Object mode constant

**Question.** Which object-mode value means hidden?

**Known.** `ON::object_mode` in `opennurbs_defines.h` gives `normal_object` 0, `hidden_object` 1, `locked_object` 2, and `idef_object` 3. `ON_3dmObjectAttributes::Read` in `opennurbs_3dm_attributes.cpp` computes the visibility of a record that has no stored flag as `m_bVisible = (Mode() != ON::hidden_object)`, where `Mode()` masks the low nibble.

**Conflict.** `HIDDEN_OBJECT_MODE` in `crates/cadmpeg-codec-rhino/src/objects.rs` is 2. The low-nibble mask and `IDEF_OBJECT_MODE` are correct. For an object-attribute minor version below 2, which has no stored visibility flag, a hidden object gives visible and a locked object gives not visible. A locked object is visible in Rhino. `resolve_identity` writes the value into `SourceIdentity::effective_visible`, and `decode.rs` propagates it through each instance level, so one locked reference hides each member below it. Nothing records the change. The unit test `fixed_visibility_and_definition_membership_use_mode_low_nibble` in `tests.rs` asserts the decoder's value and holds the defect in place.

**Need.** `HIDDEN_OBJECT_MODE` must be 1. The test must assert the openNURBS enum. `rhino_3dm.md` §9 states only "visible unless hidden" and must give the four mode values.

### NS-04. Object-attribute and layer item 28 payload

**Question.** What grammar does a stored `ON_UuidList` use?

**Known.** `ON_UuidList::Read` in `opennurbs_array.cpp` reads a chunk with `BeginRead3dmChunk( TCODE_ANONYMOUS_CHUNK, &major_version, &minor_version )`, refuses a major version other than 1, and then reads a counted archive array. The payload is a complete anonymous chunk, not a bare array. `ON_3dmObjectAttributes` item 28 in `opennurbs_3dm_attributes.cpp` and the layer item 28 in `opennurbs_layer.cpp` each read a `bool` and then one `ON_UuidList`. `rhino_3dm.md` §7.1 "An `ON_UuidList` is an anonymous version 1.0 chunk containing an archive array" states the same rule.

**Conflict.** Item 28 in `crates/cadmpeg-codec-rhino/src/objects.rs` and in `crates/cadmpeg-codec-rhino/src/settings.rs` reads a `bool` and then an `i32` count followed by that many UUIDs. The first `i32` falls on the chunk typecode `0x4000_8000`, which is above the array limit, so the count check fails. A layer that carries item 28 is dropped from the layer table, and each object on it then reports a missing layer index with no layer identity, name, or colour. An object that carries item 28 loses its complete attribute block and takes a synthetic identity. The decoder disagrees with the specification, which agrees with openNURBS.

**Need.** Item 28 must read one `ON_UuidList` chunk. This item is not PP-05 or PP-06, which cover an item ID that the built-in registry does not define. Item 28 is a named built-in item that the decoder claims to decode.

### NS-05. SubD symmetry transform

**Question.** Which values does a SubD rotate-symmetry record store for the fixed plane?

**Known.** `ON_Symmetry::Write` in `opennurbs_symmetry.cpp` writes, for `Type::Rotate`, the rotation axis and then `ON_PlaneEquation::NanPlaneEquation`, which `opennurbs_statics.cpp` defines as four quiet NaNs. The comment states that the plane is written and not read so that a file saved after 15 June 2021 stays readable by earlier code. `ON_Symmetry::Read` reads the four values and states that the fixed plane is intentionally ignored. The read is gated: `if (inner_chunk_version >= 2 && false == bNewRotatePrototype)`. A record with the prototype type value 113 carries no plane.

**Conflict.** `read_symmetry` in `crates/cadmpeg-codec-rhino/src/subd.rs` reads the rotation plane with `read_finite_values`, which refuses the first value that is not finite. Each rotate-symmetry SubD raises a malformed error, and `decode.rs` marks the object failed, so the complete control cage is discarded although each vertex, edge, and face parsed. The same function maps type 113 to type 2 and then drops the prototype gate, so a prototype record reads 32 bytes that the file does not hold. The fixture in `subd.rs` writes symmetry type 0, so no test runs the type-dependent transform grammar.

**Note.** A finiteness test is a plausibility test. The format stores non-finite padding on purpose, so the test refuses the value that the format defines.

**Need.** The rotate arm must read and discard the four values without a finiteness test, and must keep the prototype flag to gate the read. `rhino_3dm.md` §20.4 records only that a symmetry record follows minor version 2. The complete symmetry grammar must reach the specification.

### NS-06. Archive start offset

**Question.** At which offset does the 32-byte start section begin?

**Known.** `ON_BinaryArchive::Read3dmStartSection` in `opennurbs_archive.cpp` does not require the magic at offset 0. When the leading 32 bytes do not match, it moves a 32-byte window forward for up to 33554432 bytes, and on a match it stores `m_3dm_start_section_offset`. `Seek3dmChunkFromStart` takes each later offset from that value. The header comment in `opennurbs_archive.h` gives the reason: an archive saved from an application that supports linking and embedding carries a leading block that the application put there.

**Conflict.** `parse_header` in `crates/cadmpeg-codec-rhino/src/chunks.rs` compares bytes 0 through 23 against the magic and returns an invalid-header error for any other offset. `container.rs` starts the comment chunk at the constant 32, and `parse_eof` compares the stored file size against the complete input length. `detect` in `crates/cadmpeg-codec-rhino/src/lib.rs` applies the opposite rule: it scans the complete prefix with `windows(...).any(...)` and returns high confidence on a match at any offset. The codec therefore claims each file that carries a leading block, and then refuses it. No other codec receives the file.

**Need.** We must decide whether the decoder supports a nonzero start offset. Until then `detect` and `parse_header` must apply one rule. The specification states that the first 32 bytes are the start section and does not record the offset question.

### NS-07. Mesh minor gates 5 through 8

**Question.** Which condition gates the mesh fields from minor version 5 through 8?

**Known.** `ON_Mesh::Read` in `opennurbs_mesh.cpp` nests each field from minor version 4 through 8 inside one test, `minor_version >= 4 && file.ArchiveOpenNURBSVersion() >= 200606010`. The manifold bytes, the ngon block, the double vertices, and the bounding box are all inside that test.

**Conflict.** `crates/cadmpeg-codec-rhino/src/mesh.rs` applies the writer-version test to the mapping tag only and reads each later field on the minor version alone. `settings.rs` sets the writer version only from a short `TCODE_PROPERTIES_OPENNURBS_VERSION` record, so a document with an absent or long-form properties record leaves it unset. The decoder then skips the mapping-tag chunk and reads its 12-byte header as payload, and the mesh fails. openNURBS with no archive version skips each field from minor version 4 and returns a complete mesh. `rhino_3dm.md` §14 "Major 3 follows the face array with five compressed buffers for vertices," states the flattened rule as fact.

**Need.** The writer-version test must gate each field from minor version 4 through 8. The specification must carry the nested form.

### NS-08. Legacy text-style characteristics word

**Question.** How does a font characteristics word encode a style?

**Known.** `ON_Font::Internal_GetFontCharacteristicsFromUnsigned` in `opennurbs_font.cpp` reads the word as mixed radix: one set flag, then weight in base 10, style in base 4, stretch in base 10, underline in base 2, and strikethrough in base 2. `ON_Font::Style` gives `Unset` 0, `Upright` 1, and `Italic` 2. `ON_Font::ReadV5` sets the style directly and never packs a characteristics word.

**Conflict.** `parse_text_style` in `crates/cadmpeg-codec-rhino/src/presentation.rs` sets `font.characteristics` to `u32::from(italic != 0)`. Value 1 unpacks to a set flag with style digit 0, which raises an error in openNURBS and returns `Upright`. Value 0 unpacks to a clear set flag, so each field falls back to the default font. The italic flag is lost in both directions. The correct word for italic is 41. The modern path reads the stored word, so one arena field carries two encodings. `rhino_3dm.md` §20.4 promises the raw characteristics word.

**Need.** The legacy path must not synthesize a characteristics word. It must carry the italic flag in a field that states its own meaning.

### NS-09. Absent model-component index

**Question.** Which value states that a model-component index is absent?

**Known.** `ON_ModelComponent::ReadModelComponentAttributes` in `opennurbs_model_component.cpp` reads the index only when bit `0x04` is set and otherwise leaves `ON_UNSET_INT_INDEX`, which `opennurbs_defines.h` gives as -2147483647. `ON_BinaryArchive::ReadModelComponentAttributes` treats status byte 2 as a cleared index. The value -1 is a live system index: `opennurbs_3dm_attributes.cpp` sets `m_linetype_index = -1` for continuous and `m_material_index = -1` for white diffuse, and `Read3dmReferencedComponentIndex` passes a negative index through as a system component with a persistent negative index.

**Conflict.** `crates/cadmpeg-codec-rhino/src/presentation.rs` writes `component.index.unwrap_or(-1)` at each of five record sites. A record whose index attribute is absent takes the index of the continuous linetype or of the white-diffuse material. Each object that names index -1, which is the ordinary "no material" state, then joins to that record.

**Need.** An absent index must stay absent in the arena, or must carry `ON_UNSET_INT_INDEX`. The two states must not collide.

## 7. Reader strictness beyond openNURBS

ON-04 records four framing refusals. The sweep found the same shape in each
decoder. Each item below refuses a record that openNURBS reads. Each needs the
same decision that ON-04 needs: stay fatal, become a warning with recovery, or
be removed.

### RS-01. Trailing bytes in a bounded chunk

**Question.** What does a bounded chunk with unread trailing bytes mean?

**Known.** `ON_BinaryArchive::EndRead3dmChunk` in `opennurbs_archive.cpp` seeks to the declared end and returns success. Its comment names the case: a partially read chunk happens when chunks are skipped, or when old code reads a new minor version of a chunk that has added information. openNURBS relies on this behaviour for forward compatibility and never reports an error.

**Conflict.** `finish_anonymous` and `finish_anonymous_ranges` in `brep.rs`, `finish_chunk` in `subd.rs`, the record tail test in `mesh.rs`, and `finish` in `instances.rs` each return a malformed error when bytes remain. In `brep.rs` the error leaves the six sub-chunk readers, and `decode_brep` marks the object failed because the error is malformed and not an unsupported version. One added field in one sub-chunk discards the complete Brep record.

**Note.** FV-05 asks how to tell a later minor-version suffix from malformed trailing bytes. openNURBS answers that question: it never treats trailing bytes as malformed. FV-05 remains open for the grammar of the suffix.

**Need.** We must decide whether a bounded chunk may carry unread bytes. The recovery that openNURBS relies on is not available while the rule stays fatal.

### RS-02. Exact minor-version equality

**Question.** Which version fields must a decode site compare exactly?

**Known.** Each openNURBS sub-array reader for a Brep tests the major version alone: `ON_BrepVertexArray::Read`, `ON_BrepEdgeArray::Read`, `ON_BrepTrimArray::Read`, `ON_BrepLoopArray::Read`, and `ON_CurveArray::Read` each test `major_version == 1` and ignore the minor version. `ON_PolyEdgeSegment::Read` tests `1 == major_version`. `ON_MorphControl::Read` ignores the minor version on the legacy major-1 path. McNeel does raise these minor versions: the Brep face array is already at 1.1 and 1.2.

**Conflict.** `raw_array_start` and `read_children` in `brep.rs` refuse a sub-array version byte other than `0x10`. `read_faces` correctly accepts a range, and the other five readers do not. `history.rs` and `morph.rs` refuse a minor version other than 0 at nine sites. `polyedge.rs` refuses a segment minor version other than 0. The first minor-version rise on any of these arrays turns each affected record into a decode failure while openNURBS keeps reading.

**Need.** A decode site must accept a minor version that is not less than the one whose fields it reads, and must then use the trailing-byte rule of RS-01.

### RS-03. Unknown short table record

**Question.** What must a reader do with a table record typecode that it does not know?

**Known.** The openNURBS table readers switch on the child typecode and carry a default arm whose complete body is a comment: information added in future will be skipped by `file.EndRead3dmChunk()`. `opennurbs_3dm_properties.cpp` and `opennurbs_3dm_settings.cpp` both carry it. openNURBS has one short-framing rule, `ON_IsShortChunkTypecode`, which tests the short bit. There is no per-table list of permitted short typecodes.

**Conflict.** `record_is_allowed` in `crates/cadmpeg-codec-rhino/src/container.rs` permits a short record only when it is one of five enumerated typecodes, and `scan` returns a malformed error for each other short record and for a known record in an unexpected table. The five cover each short table record that exists now. A sixth short settings record turns each file from that Rhino version into a complete framing failure, so `inspect` and `decode` both refuse it. An unknown long record is skipped with a warning, which is the openNURBS behaviour.

**Need.** An unknown short record must produce a typed loss and must not stop the scan. The specification records no short-framing list.

### RS-04. Non-canonical boolean bytes

**Question.** Which byte values does a stored `bool` field allow?

**Known.** `ON_BinaryArchive::ReadBool` in `opennurbs_archive.cpp` normalizes a byte other than 0 or 1 to 1 when the archive version is below 6.0 of 24 August 2017, and cites two McNeel issues where correct file-writing code produced such bytes. From that version on it reports an error. openNURBS reads several fields with `ReadChar` into an unsigned char and applies no constraint. `ON_3dmRenderSettings::ReadV5` stores each flag as `(0 != b)` from an `int`.

**Conflict.** `bool` in `crates/cadmpeg-codec-rhino/src/chunks.rs` refuses each byte other than 0 or 1 with no archive-version condition. `flag_i32` in `document_data.rs` refuses each `i32` other than 0 or 1. The presence bytes in `mesh.rs` and the `bool_i32` reader in `views.rs` apply the same rule. A layer with one such byte is dropped from the layer table, and each object on it loses its layer identity, name, and colour.

**Need.** The archive-version condition must gate the refusal. A field that openNURBS reads with `ReadChar` must not use the strict reader.

### RS-05. Enumeration values outside the known range

**Question.** What must a reader do with an enumeration value that it does not know?

**Known.** `ON::ObjectMode(int)` and `ON::ObjectColorSource(int)` in `opennurbs_defines.cpp` clamp an unknown value to `normal_object` and to `color_from_layer`. `ON_Brep::Read` clamps `m_is_solid` outside 0 through 2 to 0. `ON_Localizer::Read` keeps `no_type`. `ON_HistoryRecord::RecordType` falls back to `history_parameters`. `ON_Symmetry` and `ON_SubDEdgeChain` apply the same shape. openNURBS does not fail a read on an enumeration value.

**Conflict.** `validate_attribute_selectors` in `objects.rs` refuses an object mode above 5 and a selector above 3, and runs after each item in the tagged stream, so one bad byte discards each of the 41 items. The mode bound of 5 has no source; `object_mode_count` is 4. `brep.rs` maps `is_solid` value 3 to a distinct body kind that no Rhino Brep carries, where openNURBS gives 0, and a unit test pins the invented value. `history.rs` refuses a record type outside 0 and 1. `morph.rs` refuses a localizer kind outside 0 through 6. `subd.rs` refuses an edge orientation above 1 where `ON_SubDEdgeChain` treats each value other than 1 as forward.

**Need.** An unknown enumeration value must clamp and record a typed loss, or must stay as stored data. It must not discard the containing record.

### RS-06. Redundant count and index agreement

**Question.** Which stored counts and indices must agree before a record decodes?

**Known.** openNURBS repairs or tolerates each of these. `ON_BrepTrimArray::Read` overwrites a trim index that does not equal its array position, and cites the McNeel issue that bogus index values exist in shipping files. `ON_SubDEdgeChain::Read` clears both arrays and returns success when the counts disagree, and reads the two arrays only when the count is above zero. `ON_PolyCurve::Read` uses the same count test only to gate an optional cleanup. `ON_InstanceDefinition::Internal_ReadV5` never compares the version 1.2 unit value against the version 1.4 unit chunk; the later chunk replaces the earlier value. `ON_Mesh::ReadFaceArray` switches on the stored face-index width alone.

**Conflict.** `positional` in `brep.rs` makes an index mismatch fatal for vertices, edges, and trims. `subd_edge_chains` in `history.rs` makes a count mismatch fatal and reads both arrays although the count may be zero, so a zero-count chain runs past the chunk end. `polyedge.rs` makes a parameter-count mismatch fatal and refuses a zero segment count. `instances.rs` refuses a record whose legacy and detailed unit values disagree, and for a custom unit it keeps the legacy scale where openNURBS keeps the chunk scale. `mesh.rs` refuses a stored face-index width that does not equal the width that the vertex count implies.

**Note.** `ON_Mesh::WriteFaceArray` selects the width from the vertex count, so a Rhino-authored file always agrees. That refusal is the least urgent of this group, and `rhino_3dm.md` §14 records the width rule.

**Need.** Each redundant field must repair or degrade, and must record a typed loss. Discarding a record loses data that the file holds.

### RS-07. Abandoned and reserved tails

**Question.** What follows the fields that a reader knows in a version 1.7 V5 instance definition?

**Known.** `ON_InstanceDefinition::Internal_ReadV5` in `opennurbs_instance.cpp` reads the optional file reference and then stops, with the comment that it is skipping the rest of what was in a 1.7 chunk because it did not work, and that the chunk will be partially read. openNURBS states that it does not know the tail and does not need it.

**Conflict.** `parse_v5` in `crates/cadmpeg-codec-rhino/src/instances.rs` asserts that the remainder is exactly one byte and that the byte is a canonical boolean, and `finish` then refuses any other remainder. A record whose abandoned tail is a different length is discarded, and with it the definition's member list, so no reference to that definition can expand.

**Need.** The tail must be skipped without an assertion about its width. `rhino_3dm.md` §21 records no trailing field.

## 8. Invented constants and thresholds

### IC-01. Curve and surface item limit

**Question.** What upper bound does a stored count have?

**Known.** openNURBS applies no upper bound to a control-point count, a knot count, or a point count. `ON_NurbsSurface::Read` adds lower bounds only.

**Conflict.** `MAX_CURVE_ITEMS` in `crates/cadmpeg-codec-rhino/src/curves.rs` is 65536. Its doc comment states points or polycurve segments, and `checked_count` in `surfaces.rs` applies it to the total control-point count of a NURBS surface, to each knot count, to the polyline point count, and to the point-cloud point count. A surface with 260 control points in each direction, which an ordinary patch or loft produces, has 67600 control points and is refused before the first pole is read. A point cloud above 65536 points is the ordinary case. The refusal is `FramingError::InvalidLength`, so a valid file reports as malformed. The bound appears in no document.

**Need.** We must decide the bound and state it. A resource bound must report as a policy refusal with a typed loss, not as a framing error.

### IC-02. Extrusion miter threshold

**Question.** Below which local z value does openNURBS stop applying a miter?

**Known.** `ON_Extrusion::m_Nz_min` in `opennurbs_beam.cpp` is 1.0/64.0. `ON_GetEndCapTransformation` applies the miter only when `N.z > ON_Extrusion::m_Nz_min` and the normal is a unit vector, and otherwise leaves the plain plane rotation, which is a flat cap. The same constant gates `IsMitered` and `SetMiterPlaneNormal`. openNURBS unitizes a non-unit normal in place and, on failure, sets a zero vector and skips the miter. `ON_Extrusion::Read` does not call `IsValid`, so the read path does not filter these normals.

**Conflict.** `MITER_Z_MINIMUM` in `crates/cadmpeg-codec-rhino/src/extrusion.rs` is 1.0e-6. Between 1.0e-6 and 1.0/64.0 the decoder applies a complete miter where openNURBS applies a flat cap: at a local z of 0.01 the factor `1.0 - 1.0 / normal.z` is -99, so the cap control points move by about one hundred times the profile size. Below 1.0e-6 the decoder returns an error and discards the extrusion where openNURBS returns a flat cap. `require_unit` refuses a non-unit normal where openNURBS unitizes it. Above 1.0/64.0 the two agree. `rhino_3dm.md` §16 states only that the miter vectors are serialized when their presence flags are false.

**Need.** The threshold must be 1.0/64.0, and the behaviour below it must be a flat cap. The specification must state when the miter applies.

### IC-03. Analytic circle selection

**Question.** Which condition selects the analytic circle representation?

**Known.** `read_circle` in `crates/cadmpeg-codec-rhino/src/curves.rs` accepts a plane x-axis whose norm is within 1.0e-10 of 1.

**Conflict.** `canonical_circle` in the same file ends its conjunction with `circle.xaxis.norm() == 1.0`, an exact float comparison on a value that the reader validated to a tolerance. A circle whose axis came out of trigonometry with a norm of 0.9999999999999999 takes the rational NURBS branch, and a circle one rotation away takes the analytic branch. The representation of the same modelling operation changes with rounding. The criterion is in no document.

**Need.** The test must use the same tolerance that `read_circle` applies, or the selection rule must be stated.

### IC-04. Quad triangulation diagonal

**Question.** Which diagonal splits a quadrilateral mesh face?

**Known.** `ON_Mesh::ConvertQuadsToTriangles` in `opennurbs_mesh.cpp` calls `ConvertNonPlanarQuadsToTriangles` with split method 1, which compares the two diagonal lengths and splits along the shorter one. The same function collapses a near-degenerate quadrilateral by removing the duplicate vertex instead of emitting a sliver triangle.

**Conflict.** `crates/cadmpeg-codec-rhino/src/mesh.rs` always splits along the 0 to 2 diagonal. A saddle-shaped quadrilateral whose 1 to 3 diagonal is shorter, which is ordinary on a render mesh of a doubly curved surface, gains a bulge in the opposite direction. A degenerate quadrilateral yields a zero-area triangle. `decode.rs` marks the tessellation byte-exact when the scale is 1.0, so a re-triangulated mesh reports as byte-exact. `rhino_3dm.md` §14 records the quadrilateral and triangle forms and states no split rule.

**Need.** The IR carries triangles, so a split is forced. The rule must match openNURBS, and the loss of quadrilateral topology must be recorded.

### IC-05. Unit scales for astronomical units, light years, and parsecs

**Question.** Which scale does each unit system value select?

**Known.** `ON::UnitScale` in `opennurbs_defines.cpp` gives the values that openNURBS uses for unit-system values 23, 24, and 25, and cites its source in `opennurbs_defines.h`.

**Conflict.** `crates/cadmpeg-codec-rhino/src/settings.rs` uses modern values for these three. A document in astronomical units round-trips with each coordinate slightly different from the openNURBS value. `rhino_3dm.md` §8.2 lists the enumeration names and records no scale factors.

**Need.** We must decide whether the codec follows openNURBS or the modern values, and state the choice. The other 22 scales agree with openNURBS.

## 9. Loss accounting

### LA-01. Integrity failures have no code

**Question.** Which loss code states that a stored checksum did not match?

**Known.** `RhinoLossCode::ALL` in `crates/cadmpeg-codec-rhino/src/loss.rs` holds 21 codes. None names a checksum, a CRC, or an integrity failure. The module doc states that codes are the gating surface, and that harness oracles and downstream tooling key on them and never on the message text.

**Conflict.** Each checksum mismatch becomes `ContainerScanDiagnostic`, which maps to `LossKind::DecodeDiagnostic`. The benign note "unknown bounded record skipped", pushed to the same vector eight lines away in `container.rs`, takes the same code, kind, severity, and category. A consumer cannot tell corruption from a record that the decoder chose to skip, because the only channel that carries the difference is the message text that it is told not to read. `LossKind::DecodeDiagnostic` has no strict floor in `crates/cadmpeg-ir/src/report.rs`, so an archive whose stored checksums do not match its bytes passes strict mode. The extrusion mismatches aggregate by message family in `decode.rs`, so distinct corrupt records merge into one note and their offsets are lost.

**Note.** ON-01 states that 13 call sites compute the checksum over the wrong range, so most mismatches on a Rhino-authored file are false. Those false mismatches and a real corruption carry the same code. Correcting ON-01 does not separate them.

**Need.** An integrity failure needs its own code and a strict floor. Until then the report cannot answer whether an archive matched its own checksums.

### LA-02. Records dropped without a loss

**Question.** Which failures must appear in the loss report?

**Known.** The crate records a degradation for a metadata record in `settings.rs` and for a view in `views.rs`, so the pattern exists.

**Conflict.** Four sites discard data with no loss and no count.

- Each table branch in `presentation.rs` pushes only on `Ok`. A material, light, linetype, hatch pattern, dimension style, text style, bitmap, or texture mapping that meets any refusal in that file is absent from the arena and cannot be told from a document that never held one. `install` takes no loss sink. `rendering_materials` returns an empty vector on failure, which in the arena equals "this object stores no rendering material", so an unreadable record reads as an ordinary inherited material.
- `read_ngons` in `mesh.rs` walks the complete record and keeps nothing. The module doc names a retained `CHANNEL_NGON_GROUP` constant that exists nowhere in the workspace. A mesh whose faces group into n-gons decodes as a success with no loss, and the grouping is gone.
- `extended_geometry_json` in `history.rs` returns the same `None` for an unsupported class and for a failed decode, because each dispatch uses `.ok()?`. Embedded geometry that fails to decode raises no warning, no loss, and no count, although the neighbouring `GeometrySink` exists to charge geometry that decoded and was not carried.
- `dimensions.rs` frames and walks the dimension-style override object and drops it, into a local warning vector that nothing reads. openNURBS installs that object as the effective style for the annotation.

**Need.** Each of these must record a typed loss. A silent absence states that the file held nothing.

### LA-03. Absence recorded as a value

**Question.** Which value states that a version-gated field was not stored?

**Known.** `ON_Interval` constructs to `ON_UNSET_VALUE` at both ends, and `ON_ObjRef::Read` leaves the evaluation intervals at that default below minor versions 1 and 2. openNURBS separates "not stored" from "stored as zero".

**Conflict.** `history.rs` fills the three object-reference evaluation intervals with 0.0 and overwrites only those the minor version supplies. `evaluation_properties` then emits `0,0` for an absent interval, which is a valid normalized parameter span at the start of an edge. The neighbouring instance-reference path uses `Option` for its version-gated fields, so the crate has the pattern and does not apply it here. `hatch.rs` gives a V5 hatch a base point of `[0.0, 0.0]`, where `ON_Hatch::Write` stores a non-origin base point in a userdata record and `ON_OBSOLETE_V5_HatchExtra` reads it back; the userdata is present in the record and the decoder does not consult it, and the fallback is wrong in exactly the case where the record carries data. `dimensions.rs` sets `arrow_position` to 0 on the modern path where the record stores a legacy arrow-fit value that openNURBS applies, so a stored "arrows outside" reports as "auto".

**Need.** An absent field must stay absent. A field that another record holds must be read from that record.

### LA-04. Derived edges dropped by file order

**Question.** Does the history table order its records so that a producer precedes its consumer?

**Known.** openNURBS states no such rule. `ON_HistoryRecord` carries no ordinal, and the writer emits the records in table order.

**Conflict.** `history.rs` resolves a dependency by stored UUID, and refuses an ambiguous producer, which is sound. It then filters each edge whose producer index is not below the consumer index, so an edge is dropped with no loss when the producing record appears later in the table.

**Note.** `crates/cadmpeg-ir/src/validate/topology.rs` makes a dependency on a record that does not precede its consumer a referential-integrity error, so an unfiltered edge would fail validation. The raw UUID lists survive in the record and in the native arena, so no stored data is lost.

**Need.** We must decide whether the IR invariant or the file is authoritative. The dropped edge must record a loss.

### NS-10. Polycurve segment join

**Question.** How does openNURBS join the segments of a polycurve into one NURBS curve?

**Known.** `ON_PolyCurve::GetNurbForm` in `opennurbs_polycurve.cpp` moves the two endpoints to their midpoint and then calls `ON_NurbsCurve::Append`. `ON_NurbsCurve::Append` in `opennurbs_nurbscurve.cpp` applies four rules. It raises the degree of the shorter segment with `IncreaseDegree` and clamps it with `ClampEnd`. It computes `dk = Knot(CVCount()-1) - c.Knot(c.Order()-2)` and adds `dk` to each knot of the appended segment. It starts the copy at `i1 = c.Order()-1` and `i2 = 1`, so it drops the first control point of the appended segment and keeps the last control point of the existing curve. The joint then holds `degree` equal knots. A 2017 guard drops a knot that would collapse a span.

**Conflict.** `merge_nurbs_segments` in `crates/cadmpeg-codec-rhino/src/curves.rs`, and its copy `c2_curve_to_nurbs` in `crates/cadmpeg-codec-rhino/src/decode.rs`, apply none of the four rules.

- It keeps both control points at the joint and skips `degree + 1` knots, so the joint holds `order` equal knots. `ON_IsValidKnotVector` in `opennurbs_knot.cpp` requires `knot[i+order-1] > knot[i]`, which that form fails. `crates/cadmpeg-ir` accepts it, because `check_knots` tests finiteness and non-decreasing order only and the length identity holds by construction. The curve therefore decodes and is refused when it is written back as an `ON_NurbsCurve`.
- It refuses segments of unequal degree with the error `polycurve segments have unequal degrees`. A polycurve of a line and an arc is the most ordinary polycurve. In `decode.rs` the refusal leaves `decode_pcurves`, and one such trim sends the complete Brep to `free_carrier_fallback`, which clears each body, face, loop, coedge, edge, and vertex.
- It concatenates the stored knots with no `dk` offset.
- It never compares the last control point of one segment with the first of the next, so a gap becomes a discontinuous curve with no warning and no loss.

**Note.** The arithmetic works out: the skip of `degree + 1` gives the length that the IR length identity needs. That is the sign that the rule was chosen to make the count agree and was not read from the reference. The unit test `recursive_compound_conversion_preserves_parent_domain_when_exact` asserts the resulting knot vector, so the form is pinned.

**Need.** The join must apply the four rules of `ON_NurbsCurve::Append`. A gap must record a loss. A refusal must cost the one trim and not the complete Brep.

### NS-11. Brep face orientation

**Question.** Which stored field gives the orientation of a Brep face against its surface?

**Known.** `ON_BrepFace::m_bRev` states whether the face normal is opposite to the surface normal. `ON_BrepRegionTopology::IsValid` in `opennurbs_brep_region.cpp` fixes the face-side direction by array position: `const int srf_dir = (fsi%2) ? -1 : 1`, and refuses a stored value that differs. `validate_regions` in `crates/cadmpeg-codec-rhino/src/brep.rs` applies the same rule and expects 1 at an even index and -1 at an odd index. The face-side direction therefore holds no per-face orientation. The bounded region index states which of the two sides holds the region.

**Conflict.** `face_sense` in `crates/cadmpeg-codec-rhino/src/decode.rs` takes the region direction in preference to `reversed_surface`, and reads `direction < 0` as reversed. Because the direction is a positional constant, the rule states that the face normal agrees with the surface normal exactly when the region lies on the surface-normal side. For a solid whose normals point out, which is the ordinary case, that inverts the sense of each face in the body. `Face::sense` in `crates/cadmpeg-ir/src/topology.rs` is defined as whether the face normal agrees with the surface normal, which is `m_bRev`. `m_bRev` is not read when region topology is present.

**Note.** `ON_BrepRegion::RegionBoundaryBrep` sets `face.m_bRev = (FS[i]->m_srf_dir < 0)`, but it builds a new Brep that is deliberately oriented into the region. It is not a rule for reading the original face. No file in the openNURBS `example_files` tree carries region topology, so no test reaches this branch. Rhino writes region topology after a Boolean or region operation.

**Need.** The face sense must come from `reversed_surface`. The region side gives the region, not the orientation.

### NS-12. V5 legacy annotation semantics

**Question.** Which values does each V5 legacy annotation record report?

**Known.** Four rules come from the openNURBS V5 readers.

- `ON_OBSOLETE_V5_DimLinear::NumericValue` in `opennurbs_internal_V2_annotation.cpp` returns `fabs(m_points[ext0].x - m_points[ext1].x)` for the aligned type and the linear type alike, and `ON_DimLinear::Measurement` applies the same rule after conversion. `ON_OBSOLETE_V5_DimLinear::Repair` holds a branch for an aligned record whose second extension point leaves the plane x axis, which states that such records exist.
- `ON_OBSOLETE_V5_Annotation::Read` selects the dimension-style index by whether the record is a text block. For a text object it prefers index 1 and reaches index 2 only when index 1 and index 0 are both negative. For a dimension or a leader it prefers index 2. It also reads the text-object indices through `Read3dmReferencedComponentIndex`, which maps a text-style archive index to a dimension-style archive index.
- `ON_OBSOLETE_V5_Annotation::Read` rewrites a text object whose justification is 0: it sets the justification to top-left and moves the plane origin up the plane y axis by the text height.
- `ON_OBSOLETE_V5_DimAngular::Dim2dPoint` resolves the arc from the stored `m_angle` and `m_radius` scalars. `ON_DimAngular::CreateFromV5DimAngular` uses those derived points for the rays and the dimension arc, and never reads `m_points[3]`.

**Conflict.** `crates/cadmpeg-codec-rhino/src/dimensions.rs` differs on each.

- It measures the aligned type with `hypot` of both components and the linear type with the absolute x component. It does not rotate the plane, so one `Definition::Linear` field carries two measurement conventions with nothing in the record to separate them.
- It selects the dimension-style index by index position alone, with no text-object test, so a text object whose index 2 is a stale slot reports that slot. It emits an unmapped text-style index in a field that holds dimension-style indices from other records.
- It reports the stored justification and the stored plane origin, so a text object whose justification is 0 reports an origin one text height low and a justification value that means undefined.
- It resolves the angular arc from the stored points and takes the measurement from the stored scalar, so the reported directions and the reported measurement can contradict each other.

**Need.** Each rule must follow the V5 reader, or the neutral record must state which convention it carries.

### NS-13. View clipping-plane depth

**Question.** Which stored depth values enable depth clipping?

**Known.** `ON_ClippingPlaneInfo::Read` in `opennurbs_planesurface.cpp` enables depth clipping for minor version 1 and 2 only when the depth is not less than zero **and** is not `ON_UNSET_POSITIVE_FLOAT`, which `opennurbs_defines.h` gives as 1.234321e+38. Otherwise it disables depth clipping and sets the depth to zero. `rhino_3dm.md` §13.4 "nonnegative depth other than the unset positive value enables depth clipping;" states the same rule for the clipping-plane object record.

**Conflict.** `crates/cadmpeg-codec-rhino/src/views.rs` tests only that the depth is not less than zero for the view attributes list. A record that stores the unset marker reports depth clipping enabled with a depth near 1.2e38 times the scale, where Rhino reports depth clipping disabled with depth zero. The specification records the rule for the sibling record and §20.4 does not state it for this one.

**Need.** The view path must apply the unset-marker test that the object path applies.

## 10. Write side

### WR-01. Object-attribute item order

**Question.** In which order must a writer emit object-attribute items?

**Known.** `ON_3dmObjectAttributes::Internal_ReadV5` in `opennurbs_3dm_attributes.cpp` is one pass of ascending tests, `if ( 1 == itemid )` through `if ( 41 == itemid )`. After it reads item 13 it can match only item 14 and above. `Internal_WriteV5` emits the items in ascending order, so item 11 goes before item 13.

**Conflict.** `crates/cadmpeg-codec-rhino/src/writer.rs` emits item 6 and item 13 inside the colour branch and item 11 after it. An object that has a colour and a visibility flag gives the stream 1, 6, 13, 11, 0. openNURBS reads item 13, then reads item id 11, matches no later test, leaves the loop, and reports an internal error. `m_bVisible` keeps its default of true, so a hidden object opens visible in Rhino. An object with no colour gives 11, 0, which is correct, so the defect needs a coloured object.

**Note.** The reader in `objects.rs` is a `match` in a loop and accepts any order, so the round-trip tests pass. Both ends have the same author.

**Need.** The writer must emit the items in ascending order.

### WR-02. Seam trim type

**Question.** Which trim type does an edge that two trims of one loop share take?

**Known.** `ON_BrepTrim::TYPE` in `opennurbs_brep.h` gives `mated` for an edge that no other trim of the same loop uses, and `seam` for an edge that exactly one other trim of the same loop also uses. `ON_Brep::IsValidTrim` refuses a Brep whose two trims of one loop share an edge and whose type is not `seam`.

**Conflict.** `multi_face_brep_payload` in `writer.rs` selects `boundary` for one use and `mated` for each other count, and never tests whether the two uses fall in one loop. The single-face path holds that test and the multi-face path does not. A closed NURBS face, such as a cylinder wall with a cap, writes both seam trims as `mated`. `ON_Brep::Read` does not call `IsValid`, so the file loads and Rhino reports bad geometry on the first operation that checks.

**Need.** The writer must emit `seam` for an edge that two trims of one loop share.

### WR-03. Solid orientation

**Question.** What does the stored solid state mean?

**Known.** `opennurbs_brep.h` states that `m_is_solid` must never be set directly, and gives 0 for unset, 1 for a solid whose normals point out, 2 for a solid whose normals point in, and 3 for not solid. `ON_Brep::SolidOrientation` returns the stored value and recomputes only for 0. `ON_Brep::Write` writes the value that `IsSolid` and `SolidOrientation` computed.

**Conflict.** `writer.rs` writes 1 for each `BodyKind::Solid`. The IR states that a solid is a closed volume-bounding body and promises nothing about the normal direction, and the writer accepts a reversed sense on each face. A solid whose shells face inward is written as normals pointing out, so Rhino reports a positive volume for an inverted solid and each operation that keys on the orientation acts in the wrong direction. Writing 0 would make openNURBS compute the value.

**Need.** The writer must write 0, or must compute the orientation.

### WR-04. Loop type

**Question.** What selects the outer loop of a face?

**Known.** `opennurbs_brep.h` defines `outer` as a loop whose 2d curves form a simple closed curve with counterclockwise orientation, and `inner` as clockwise. `ON_Brep::ComputeLoopType` derives the type from the signed 2d area, under a comment that states the function must always compute the type from the 2d trim geometry and must never return the stored value.

**Conflict.** `writer.rs` selects `outer` for the first loop in the list and `inner` for each other loop, at both the single-face and the multi-face site. It ignores `LoopBoundaryRole`, which the IR carries and which the step, iges, catia, and creo decoders fill. `check_loops` in `crates/cadmpeg-ir` refuses more than one explicit outer loop and does not require the outer loop to be first, so an IR whose loop list starts with the inner loop validates and writes the roles swapped, and Rhino then trims the face to the hole. The writer also never tests the sign of the 2d area. `plane_uv` builds the uv frame about the surface normal while the IR ring order follows the face normal, so a face with a reversed sense has a clockwise outer loop in uv space and must be `inner` by the openNURBS definition.

**Note.** A Rhino-to-Rhino round trip cannot show this, because the Rhino decoder sets each boundary role to unspecified and emits the loops in trim order.

**Need.** The loop type must come from `LoopBoundaryRole`, or from the sign of the 2d area.

### WR-05. Stored writer version

**Question.** Which openNURBS version does a written archive state?

**Known.** `ON_BinaryArchive::Write3dmStartSection` in `opennurbs_archive.cpp` stores `ON::Version()`. `Read3dmProperties` restores it, and each read gate of the form `ArchiveOpenNURBSVersion() >= X` uses it. Modern values are packed and are near 2.4e9. The value 200712190 is the earlier year-month-day form and states December 2007.

**Conflict.** `writer.rs` stores the constant 200712190. Each gate for a later version is then false against a written archive, and each legacy workaround is true, whatever archive version the file states. The gate in `ON_3dmObjectAttributes::Read` is `Archive3dmVersion() >= 5 && ArchiveOpenNURBSVersion() >= 200712190`, which the constant meets exactly and with no margin. A lower value would send each object's attributes to the V4 fixed-structure path. As the writer grows to cover SubD, text styles, materials, and dimension styles, each new class takes its legacy branch with no diagnostic. No document records the constant.

**Need.** We must decide the version that a written archive states, and record it.

### WR-06. Written chunk lengths and versions

**Question.** Which chunk versions and array lengths does openNURBS write for a target archive version?

**Known.** `ON_Brep::Write` selects Brep minor version 2 for archive version 50 and below and minor version 3 above it, and minor version 3 carries the region topology. `ON_BrepFaceArray::Write` selects face-array minor version 2 for archive version 70 and above and 1 below it; minor version 1 carries the per-face UUIDs and minor version 2 the per-face colours. `ON_Brep::Write` writes exactly `face_count` presence bytes into each mesh-side chunk, and `ON_Brep::Read` reads exactly that many.

**Conflict.** `writer.rs` writes the Brep chunk version 3.2 and the face-array version 1.0 for each archive version, so the version bytes carry no archive-version dependence and the writer can never carry region topology, face identity, or face colour. It also writes `model.faces.len() + 1` presence bytes into each of the two mesh-side chunks. openNURBS then finds a partially read chunk, skips the checksum test, and reports a warning for each written Brep. The surplus byte carries no data.

**Need.** The chunk versions must follow the target archive version. The presence arrays must hold one byte for each face.

### LA-05. Edge parameter range direction

**Question.** How does a neutral edge state that its proxy curve is reversed?

**Known.** `ON_BrepEdge::Read` in `opennurbs_brep_io.cpp` sets the proxy curve with an ascending domain, calls `Reverse` when the stored flag is set, and then sets the domain. `Domain()` is ascending. openNURBS never stores a descending interval.

**Conflict.** `edge_param_range` in `crates/cadmpeg-codec-rhino/src/decode.rs` swaps the two ends of the proxy domain when the reversal flag is set, so the emitted range descends. `crates/cadmpeg-ir/src/validate/carriers_parameterization.rs` requires `start <= end` whenever the edge has a curve, and `writer.rs` refuses a range whose start is not below its end. The draft commit tests identities and references only, so the value passes decode and fails later at `cadmpeg validate` or on write-back, and it costs the complete body. The decoder, the validator, and the writer hold three positions and no document records the disagreement.

**Note.** The reversal is rare in the openNURBS `example_files` tree, and the trim path makes the opposite choice for the same wire pair: it reports the trim evaluation domain where the emitted knots use the proxy domain. The two conventions differ in a way that `writer.rs` can satisfy only by accident.

**Need.** We must choose one representation for a reversed proxy. The decoder, the validator, and the writer must agree.

## 11. Further observations

These need a decision but do not each need an item. Each was read at the source
and each disagrees with openNURBS.

- `stage_brep` in `decode.rs` discards `writer_version`, so it cannot apply the openNURBS rule that an archive written before 2 October 2002 must ignore the stored solid state. `brep.rs` already threads that value for the trim and edge gates.
- A Brep whose archive stores no solid state reports as a sheet. openNURBS treats the absent value as unknown and computes it.
- `parse_material` in `presentation.rs` keeps the stored transparent colour. `ON_Material::Internal_ReadV5` replaces it with the diffuse colour for an archive written before 1 December 2009 when the stored value is 128,128,128, on the stated ground that the value was a wrong default.
- `parse_text_style` in `presentation.rs` reports the V5 description string as a PostScript name. `ON_Font::ReadV5` adopts it only when the name is not an Apple font name and the archive runtime or version allows it. `parse_text_style` cannot reach the archive runtime, so the gate is not available.
- `revolution_nurbs` in `surfaces.rs` does not move a singular pole onto the axis. `ON_RevSurface::GetNurbForm` sets each control point of a pole row to the exact axis point, under the comment that it makes singular points spot on.
- Group member links in `presentation.rs` go to the first record that holds a repeated group index, and the light merge keeps the first payload for a repeated light identity. `settings.rs` warns for a repeated layer index, so the crate holds the pattern.
- The `ON_Color` alpha channel is inverted: `opennurbs_color.h` states that 0 means opaque. `rhino_3dm.md` §4 gives the four direct colour bytes and states neither the channel order nor the inverted alpha.
- `rhino_3dm.md` §6.2 gives the object record type as `0x82a00071` and the object record end as `0x82a0007f`. `opennurbs_3dm.h` gives `TCODE_INTERFACE` as `0x02000000`, so the values are `0x82000071` and `0x8200007f`. `objects.rs` uses the correct values, so this is a specification defect only. The typecode registry is the artifact that carries the record identity.
- `transform_new_entities` in `decode.rs` transforms a free point only when the emitter added no body, where it keys curves and surfaces by ownership. An emitter that produces a body and a genuinely free point leaves that point untransformed.
