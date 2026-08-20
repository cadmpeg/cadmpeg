<!-- Generated from docs/layouts/f3d.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `f3d` record layouts

Source of truth: [`docs/formats/f3d.md`](../../docs/formats/f3d.md).
Table source: `docs/layouts/f3d.toml`.

Covers the fixed Design-segment headers, parameter-owner prefix, Draft scope frames, and body-map prefix, the named solid-primitive prologue,
the versioned Hole point-data and direct-selection prefixes,
the ParaMesh entry-name, container-GUID, body graph, collection, texture table,
feature scope, current and shifted Extrude operation and extent sections,
wrapper, and Scene records,
the compact and ten-reference `CoilPrimitive` prologues and matrix blocks, the
compact `Loft` prefix and nested profile-region frames, the class-418
`SplitFace` prefix, the grouped recipe-reference prefix, the three `Combine`
operation prologues and cross-document selector, the axial `Assemble` carrier
and selector prefixes, the non-axial assembly-operation operand-path locator run,
locator, and wrapper, and the sheet-metal `EdgeFlange` fixed operation section
(§3.1), plus the `Decal` scope, image-record prefixes, current sketch-container visibility member, and the grouped identity `Component Insert` frames. ASM stream records are tabulated in `docs/layouts/asm.toml`. Protein page records are tabulated in `docs/layouts/protein.toml`.
Container and manifest layers are text grammars and are listed under "Not
tabulated".

## `indexed_design_record_header`

Spec §3.1 · layout: byte offsets · size: 11 B

The 11-byte size is the spec's own "eleven-byte indexed header". §3.1 states the segment's integers are little-endian ("a nonempty contiguous sequence of little-endian i32 values").

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `class_tag_length` | `u32` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 4 | 3 | `class_tag` | `bytes[3]` | little | spec | a three-digit ASCII dynamic-class tag |
| 7 | 4 | `record_index` | `u32` | little | spec | then `u32 record_index` |

## `sketch_container_visibility_member_prefix`

Spec §3.1 · layout: byte offsets · size: 37 B

Offsets are relative to the typed Geometry member's indexed header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `class_tag_length` | `u32` | little | spec | The member starts with `u32 3` |
| 4 | 3 | `class_tag` | `bytes[3]` | little | spec | its three-digit dynamic class tag |
| 7 | 8 | `entity_suffix` | `u64` | little | spec | the owning u64 sketch entity suffix |
| 15 | 4 | `zero_run` | `bytes[4]` | little | spec | Four zero bytes and one same-segment marked owner reference follow |
| 19 | 11 | `owner_reference` | `bytes[11]` | little | spec | one same-segment marked owner reference follow |
| 30 | 4 | `stream_ordinal` | `u32` | little | spec | u32 at member offset 30 |
| 34 | 1 | `reserved_zero` | `u8` | little | spec | Byte 34 is zero |
| 35 | 1 | `visible` | `u8` | little | spec | visibility flag is at member offset 35 |
| 36 | 1 | `tail_marker` | `u8` | little | spec | byte 36 is `01` |

Cross-checked against code:

- `crates/cadmpeg-codec-f3d/src/design/decode/sketch.rs` — The decoder selects the member by this type GUID before reading this prefix.

## `design_decal_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 44 B

Offsets are relative to the Decal scope's primary indexed header. The remaining scope payload follows this fixed prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/decal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | scope offsets 11 through 20 |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Ten zero bytes occupy scope offsets 11 through 20 |
| 21 | 5 | `asset_reference` | `bytes[5]` | little | spec | A marked image-asset record reference occurs at offset 21 |
| 26 | 6 | `asset_reference_zero_run` | `bytes[6]` | little | spec | with six trailing zero bytes |
| 32 | 1 | `mapping_mode` | `u8` | little | spec | Mapping mode `0x60` at offset 32 |
| 33 | 5 | `target_group_reference` | `bytes[5]` | little | spec | A marked target-group reference occurs at offset 33 |
| 38 | 6 | `target_reference_zero_run` | `bytes[6]` | little | spec | with six trailing zero bytes |

## `design_decal_image_asset_record`

Spec §3.1 · layout: byte offsets · size: 30 B

Complete primary Decal image-asset record. The image-name record begins at byte 30.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/decal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The primary record is 30 bytes |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | Eight zero bytes occupy offsets 11 through 18 |
| 19 | 5 | `design_entity_suffix_reference` | `bytes[5]` | little | spec | a marked Fusion Design entity suffix at offset 19 |
| 24 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes at offsets 24 through 29 |

## `design_decal_image_name_prefix`

Spec §3.1 · layout: byte offsets · size: 25 B

Fixed prefix through the LP-UTF16 code-unit count. The variable UTF-16LE basename starts at byte 25.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/decal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | has the primary record index plus one |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Ten zero bytes occupy its offsets 11 through 20 |
| 21 | 4 | `asset_name_code_unit_count` | `u32` | little | spec | An LP-UTF16 archive-entry basename begins at offset 21 |

## `design_parameter_legacy_287_prefix`

Spec §3.1 · layout: byte offsets · size: 45 B

Offsets are relative to the class-287 parameter header through the compact expression length. The variable expression is followed by the exact five-byte trailer 00 00 00 00 00 or 00 00 00 01 00.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by fifteen zero bytes |
| 11 | 15 | `zero_run_15` | `bytes[15]` | little | spec | The class-287 prefix uses the compact-owned fifteen-byte zero run. |
| 26 | 4 | `source_ordinal` | `u32` | little | spec | `u32 source_ordinal` at offset 26 |
| 30 | 1 | `owner_marker` | `u8` | little | spec | `u8 1 + u32 owner_record_index` at offsets 30 and 31 · value `1` |
| 31 | 4 | `owner_record_index` | `u32` | little | spec | `u8 1 + u32 owner_record_index` at offsets 30 and 31 |
| 35 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes at offsets 35 through 40 |
| 41 | 4 | `expression_length` | `u32` | little | spec | the expression at offset 41 |

## `design_parameter_legacy_287_tail`

Spec §3.1 · layout: byte offsets · size: 12 B

This tail is relative to the end of the variable LP-UTF16 name and evaluated scalar.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `tail_prefix` | `bytes[2]` | little | spec | the twelve-byte tail `00 01 AF 00 00 00 00 00 00 00 00 00` · value `[0, 1]` |
| 2 | 1 | `family_marker` | `u8` | little | spec | the twelve-byte tail `00 01 AF 00 00 00 00 00 00 00 00 00` · value `175` |
| 3 | 9 | `zero_run_9` | `bytes[9]` | little | spec | the twelve-byte tail `00 01 AF 00 00 00 00 00 00 00 00 00` |

## `design_parameter_owner_prefix`

Spec §3.1 · layout: byte offsets · size: 39 B

Offsets are relative to the parameter-owner primary header. The selected scalar envelope starts at offset 39.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | an eleven-byte indexed header |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | eight zero bytes |
| 19 | 1 | `one_marker` | `u8` | little | spec | `01 + u32 1` |
| 20 | 4 | `one_value` | `u32` | little | spec | `01 + u32 1` |
| 24 | 1 | `scope_marker` | `u8` | little | spec | `01 + u32 scope_record_index` |
| 25 | 4 | `scope_record_index` | `u32` | little | spec | `01 + u32 scope_record_index` |
| 29 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes |
| 35 | 4 | `local_ordinal` | `u32` | little | spec | `u32 local_ordinal` |

## `design_parameter_owner_legacy_68`

Spec §3.1 · layout: byte offsets · size: 68 B

Offsets are relative to the legacy parameter-owner primary header. The scope and scalar lanes are absent.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte indexed header |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | eight zero bytes |
| 19 | 1 | `first_marker` | `u8` | little | spec | `u8 1` |
| 20 | 13 | `zero_run_13` | `bytes[13]` | little | spec | thirteen zero bytes |
| 33 | 1 | `parameter_marker` | `u8` | little | spec | `u8 1 + u32 parameter_record_index` |
| 34 | 4 | `parameter_record_index` | `u32` | little | spec | `u8 1 + u32 parameter_record_index` |
| 38 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes |
| 44 | 4 | `owned_ordinal` | `u32` | little | spec | `u32 owned_ordinal` |
| 48 | 7 | `zero_run_7` | `bytes[7]` | little | spec | seven zero bytes |
| 55 | 1 | `companion_marker` | `u8` | little | spec | `u8 1 + u32 companion_record_index` |
| 56 | 4 | `companion_record_index` | `u32` | little | spec | `u8 1 + u32 companion_record_index` |
| 60 | 8 | `zero_run_8_tail` | `bytes[8]` | little | spec | eight zero bytes |

## `design_parameter_owner_legacy_88`

Spec §3.1 · layout: byte offsets · size: 88 B

Offsets are relative to the legacy parameter-owner primary header. The scalar and local-ordinal lanes are absent.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte indexed header |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | the same prefix through `owned_ordinal` |
| 19 | 1 | `first_marker` | `u8` | little | spec | the same prefix through `owned_ordinal` |
| 20 | 13 | `zero_run_13` | `bytes[13]` | little | spec | the same prefix through `owned_ordinal` |
| 33 | 1 | `parameter_marker` | `u8` | little | spec | the same prefix through `owned_ordinal` |
| 34 | 4 | `parameter_record_index` | `u32` | little | spec | the same prefix through `owned_ordinal` |
| 38 | 6 | `zero_run_6` | `bytes[6]` | little | spec | the same prefix through `owned_ordinal` |
| 44 | 4 | `owned_ordinal` | `u32` | little | spec | the same prefix through `owned_ordinal` |
| 48 | 4 | `zero_run_4` | `bytes[4]` | little | spec | four zero bytes |
| 52 | 1 | `scope_marker` | `u8` | little | spec | `u8 1 + u32 scope_record_index` |
| 53 | 4 | `scope_record_index` | `u32` | little | spec | `u8 1 + u32 scope_record_index` |
| 57 | 8 | `zero_run_8_between_scopes` | `bytes[8]` | little | spec | eight zero bytes |
| 65 | 1 | `companion_marker` | `u8` | little | spec | `u8 1 + u32 companion_record_index` |
| 66 | 4 | `companion_record_index` | `u32` | little | spec | `u8 1 + u32 companion_record_index` |
| 70 | 7 | `zero_run_7` | `bytes[7]` | little | spec | seven zero bytes |
| 77 | 1 | `repeated_scope_marker` | `u8` | little | spec | `u8 1 + u32 scope_record_index` with a six-byte zero trailer |
| 78 | 4 | `repeated_scope_record_index` | `u32` | little | spec | `u8 1 + u32 scope_record_index` with a six-byte zero trailer |
| 82 | 6 | `zero_run_6_tail` | `bytes[6]` | little | spec | a six-byte zero trailer |

## `thicken_class_347_scope_frame`

Spec §3.1 · layout: byte offsets · size: 291 B

Offsets are relative to the primary indexed header. The paired class-258 header begins at offset 291; the class pair and frame length admit this form.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | primary frame |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Ten zero bytes occupy offsets 11 through 20 |
| 21 | 4 | `feature_form` | `u32` | little | spec | u32 `4` is at offset 21 · value `4` |
| 25 | 4 | `group_form` | `u32` | little | spec | u32 `1` is at offset 25 · value `1` |
| 29 | 11 | `group_reference` | `bytes[11]` | little | spec | A marked reference at offset 29 names the first ordered group |
| 40 | 2 | `scalar_prefix` | `bytes[2]` | little | spec | Bytes 40 and 41 are `01 01` · value `[1, 1]` |
| 42 | 11 | `scalar_reference` | `bytes[11]` | little | spec | the marked reference at offset 42 names the last ordered scalar |
| 53 | 4 | `auxiliary_count` | `u32` | little | spec | Offset 53 stores u32 `1` · value `1` |
| 57 | 11 | `auxiliary_reference` | `bytes[11]` | little | spec | a marked auxiliary reference at offset 57 |
| 68 | 8 | `zero_run_8` | `bytes[8]` | little | spec | Eight zero bytes at offsets 68 through 75 |
| 76 | 4 | `guid_code_unit_count` | `u32` | little | spec | its count is u32 `36` at offset 76 · value `36` |
| 80 | 72 | `guid` | `bytes[72]` | little | spec | a 36-code-unit LP-UTF16 GUID at offset 80 |
| 152 | 3 | `zero_run_3` | `bytes[3]` | little | spec | Three zero bytes at offsets 152 through 154 |
| 155 | 4 | `reference_count` | `u32` | little | spec | the three-entry ordered reference table at offset 155 · value `3` |
| 159 | 11 | `group_reference_entry` | `bytes[11]` | little | spec | the three-entry ordered reference table at offset 155 |
| 170 | 11 | `member_reference_entry` | `bytes[11]` | little | spec | the three-entry ordered reference table at offset 155 |
| 181 | 11 | `scalar_reference_entry` | `bytes[11]` | little | spec | the three-entry ordered reference table at offset 155 |
| 192 | 4 | `history_state_id` | `u32` | little | spec | The current history-state identity is at offset 192 |
| 196 | 4 | `kind_code_unit_count` | `u32` | little | spec | The scope kind is u32 `7` at offset 196 · value `7` |
| 200 | 14 | `kind` | `bytes[14]` | little | spec | followed by `Thicken` at offset 200 |
| 214 | 4 | `feature_ordinal` | `u32` | little | spec | the feature ordinal follows at offset 214 |
| 245 | 4 | `previous_history_state_id` | `u32` | little | spec | the preceding history-state identity at offset 245 |

Unstated regions:

- `218..245` (27 B): The generic fixed scope tail precedes the preceding history-state identity.
- `249..291` (42 B): The generic fixed scope tail closes immediately before the paired header at offset 291.

## `design_mirror_scope_class413_tail`

Spec §3.1 · layout: byte offsets · size: 77 B

Offsets are relative to the first byte after the UTF-16LE Mirror kind. The variable class-413 reference-table prefix precedes this fixed tail.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `feature_ordinal` | `u32` | little | spec | offset 0 stores the feature ordinal |
| 31 | 4 | `previous_history_state` | `u32` | little | spec | offset 31 stores the preceding-history state |
| 35 | 4 | `scalar_marker` | `u32` | little | spec | offset 35 stores u32 marker `89` · value `89` |
| 39 | 8 | `stitch_tolerance` | `f64` | little | spec | offset 39 stores the positive stitch tolerance as an f64 in source centimetres |
| 47 | 4 | `repeated_scalar_marker` | `u32` | little | spec | offset 47 repeats marker `89` · value `89` |
| 51 | 13 | `first_reference` | `bytes[13]` | little | spec | Offset 51 stores a marked reference to scope index plus two followed by two zero bytes |
| 64 | 13 | `second_reference` | `bytes[13]` | little | spec | offset 64 stores a marked reference to scope index plus one followed by two zero bytes |

Unstated regions:

- `4..31` (27 B): The class-413 tail reserves the span before the preceding-history state.

## `design_draft_scope_class318_compact`

Spec §3.1 · layout: byte offsets · size: 336 B

Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 336.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Each `byte_offset` is the start of the primary 11-byte indexed header |
| 171 | 4 | `reference_count` | `u32` | little | spec | +171: u32 reference_count=6 |
| 175 | 66 | `references` | `bytes[66]` | little | spec | Six consecutive entries. Each entry is u8 one, u32 record index, and six zero bytes; the record-index value starts at offset 176 + 11i. |
| 241 | 4 | `current_history_state` | `u32` | little | spec | +241: u32 current_history_state |
| 245 | 4 | `kind_code_unit_count` | `u32` | little | spec | +245: u32 code_unit_count=5 |
| 249 | 10 | `kind` | `bytes[10]` | little | spec | +249: UTF-16LE Draft |
| 259 | 4 | `feature_ordinal` | `u32` | little | spec | +259: u32 feature_ordinal |
| 290 | 4 | `previous_history_state` | `u32` | little | spec | +290: u32 previous_history_state |

Unstated regions:

- `11..171` (160 B): The variable Draft prologue precedes the fixed reference table.
- `263..290` (27 B): The Draft frame carries an unassigned span before the preceding-history state.
- `294..336` (42 B): The fixed frame tail before the paired indexed header is not assigned.

## `design_draft_scope_class318_shifted`

Spec §3.1 · layout: byte offsets · size: 340 B

Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 340.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Each `byte_offset` is the start of the primary 11-byte indexed header |
| 171 | 4 | `reserved_zero` | `u32` | little | spec | +171: u32 0 · value `0` |
| 175 | 4 | `reference_count` | `u32` | little | spec | +175: u32 reference_count=6 |
| 179 | 66 | `references` | `bytes[66]` | little | spec | Six consecutive entries. Each entry is u8 one, u32 record index, and six zero bytes; the record-index value starts at offset 180 + 11i. |
| 245 | 4 | `current_history_state` | `u32` | little | spec | +245: u32 current_history_state |
| 249 | 4 | `kind_code_unit_count` | `u32` | little | spec | +249: u32 code_unit_count=5 |
| 253 | 10 | `kind` | `bytes[10]` | little | spec | +253: UTF-16LE Draft |
| 263 | 4 | `feature_ordinal` | `u32` | little | spec | +263: u32 feature_ordinal |
| 294 | 4 | `previous_history_state` | `u32` | little | spec | +294: u32 previous_history_state |

Unstated regions:

- `11..171` (160 B): The variable Draft prologue precedes the reserved zero and fixed reference table.
- `267..294` (27 B): The Draft frame carries an unassigned span before the preceding-history state.
- `298..340` (42 B): The fixed frame tail before the paired indexed header is not assigned.

## `design_draft_scope_class318_legacy`

Spec §3.1 · layout: byte offsets · size: 373 B

Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 373.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Each `byte_offset` is the start of the primary 11-byte indexed header |
| 171 | 4 | `reserved_zero` | `u32` | little | spec | +171..+174: u32 0 · value `0` |
| 175 | 4 | `reference_count` | `u32` | little | spec | +175: u32 reference_count=9 |
| 179 | 99 | `references` | `bytes[99]` | little | spec | Nine consecutive entries. Each entry is u8 one, u32 record index, and six zero bytes; the record-index value starts at offset 180 + 11i. |
| 278 | 4 | `current_history_state` | `u32` | little | spec | +278: u32 current_history_state |
| 282 | 4 | `kind_code_unit_count` | `u32` | little | spec | +282: u32 code_unit_count=5 |
| 286 | 10 | `kind` | `bytes[10]` | little | spec | +286: UTF-16LE Draft |
| 296 | 4 | `feature_ordinal` | `u32` | little | spec | +296: u32 feature_ordinal |
| 327 | 4 | `previous_history_state` | `u32` | little | spec | +327: u32 previous_history_state |

Unstated regions:

- `11..171` (160 B): The variable Draft prologue precedes the reserved zero and fixed reference table.
- `300..327` (27 B): The Draft frame carries an unassigned span before the preceding-history state.
- `331..373` (42 B): The fixed frame tail before the paired indexed header is not assigned.

## `design_draft_scope_class372`

Spec §3.1 · layout: byte offsets · size: 340 B

Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 340.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Each `byte_offset` is the start of the primary 11-byte indexed header |
| 171 | 4 | `reserved_zero` | `u32` | little | spec | +171: u32 0 · value `0` |
| 175 | 4 | `reference_count` | `u32` | little | spec | +175: u32 reference_count=6 |
| 179 | 66 | `references` | `bytes[66]` | little | spec | +179 + 11i: reference entry |
| 245 | 4 | `current_history_state` | `u32` | little | spec | +245: u32 current_history_state |
| 249 | 4 | `kind_code_unit_count` | `u32` | little | spec | +249: u32 code_unit_count=5 |
| 253 | 10 | `kind` | `bytes[10]` | little | spec | +253: UTF-16LE Draft |
| 263 | 4 | `feature_ordinal` | `u32` | little | spec | +263: u32 feature_ordinal |
| 294 | 4 | `previous_history_state` | `u32` | little | spec | +294: u32 previous_history_state |

Unstated regions:

- `11..171` (160 B): The variable Draft prologue precedes the reserved zero and fixed reference table.
- `267..294` (27 B): The Draft frame carries an unassigned span before the preceding-history state.
- `298..340` (42 B): The fixed frame tail before the paired indexed header is not assigned.

## `design_draft_scope_class393`

Spec §3.1 · layout: byte offsets · size: 339 B

Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 339.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Each `byte_offset` is the start of the primary 11-byte indexed header |
| 171 | 4 | `reserved_zero` | `u32` | little | spec | +171: u32 0 · value `0` |
| 175 | 4 | `reference_count` | `u32` | little | spec | +175: u32 reference_count=6 |
| 179 | 66 | `references` | `bytes[66]` | little | spec | +179 + 11i: reference entry |
| 245 | 4 | `current_history_state` | `u32` | little | spec | +245: u32 current_history_state |
| 249 | 4 | `kind_code_unit_count` | `u32` | little | spec | +249: u32 code_unit_count=5 |
| 253 | 10 | `kind` | `bytes[10]` | little | spec | +253: UTF-16LE Draft |
| 263 | 4 | `feature_ordinal` | `u32` | little | spec | +263: u32 feature_ordinal |
| 293 | 4 | `previous_history_state` | `u32` | little | spec | +293: u32 previous_history_state |

Unstated regions:

- `11..171` (160 B): The variable Draft prologue precedes the reserved zero and fixed reference table.
- `267..293` (26 B): The 76-byte tail leaves a one-byte shorter span before the preceding-history state.
- `297..339` (42 B): The fixed frame tail before the paired indexed header is not assigned.

## `design_draft_scope_class448`

Spec §3.1 · layout: byte offsets · size: 340 B

Offsets are relative to the primary Draft indexed header. The paired indexed header begins at offset 340.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Each `byte_offset` is the start of the primary 11-byte indexed header |
| 171 | 4 | `reserved_zero` | `u32` | little | spec | +171: u32 0 · value `0` |
| 175 | 4 | `reference_count` | `u32` | little | spec | +175: u32 reference_count=6 |
| 179 | 66 | `references` | `bytes[66]` | little | spec | +179 + 11i: reference entry |
| 245 | 4 | `current_history_state` | `u32` | little | spec | +245: u32 current_history_state |
| 249 | 4 | `kind_code_unit_count` | `u32` | little | spec | +249: u32 code_unit_count=5 |
| 253 | 10 | `kind` | `bytes[10]` | little | spec | +253: UTF-16LE Draft |
| 263 | 4 | `feature_ordinal` | `u32` | little | spec | +263: u32 feature_ordinal |
| 294 | 4 | `previous_history_state` | `u32` | little | spec | +294: u32 previous_history_state |

Unstated regions:

- `11..171` (160 B): The variable Draft prologue precedes the reserved zero and fixed reference table.
- `267..294` (27 B): The Draft frame carries an unassigned span before the preceding-history state.
- `298..340` (42 B): The fixed frame tail before the paired indexed header is not assigned.

## `design_hole_point_data_v1_prefix`

Spec §3.1 · layout: byte offsets · size: 97 B

Offsets are relative to the version-one Hole point-data primary indexed header. The counted non-null reference run follows the fixed prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | the point-data primary indexed header |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | eight zero bytes at offsets `11` through `18` |
| 19 | 1 | `leading_block_presence` | `u8` | little | spec | a leading-block presence byte at offset `19` |
| 20 | 1 | `property_block_presence` | `u8` | little | spec | a property-block presence byte at offset `20` |
| 21 | 4 | `bounding_box_index` | `u32` | little | spec | a bounding-box index at offset `21` |
| 25 | 24 | `position` | `f64[3]` | little | spec | The position triple is at offset `25` |
| 49 | 24 | `direction` | `f64[3]` | little | spec | the model-space direction triple at offset `49` |
| 73 | 16 | `point_parameters` | `f64[2]` | little | spec | the two construction parameters at offset `73` |
| 89 | 4 | `reference_type` | `u32` | little | spec | `refType` at offset `89` |
| 93 | 4 | `input_count` | `u32` | little | spec | the counted input-reference count at offset `93` |

## `design_hole_point_data_v4_prefix`

Spec §3.1 · layout: byte offsets · size: 122 B

Offsets are relative to the version-four Hole point-data primary indexed header. The counted non-null reference run follows the fixed prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | the point-data primary indexed header |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | eight zero bytes at offsets `11` through `18` |
| 19 | 1 | `leading_block_presence` | `u8` | little | spec | a leading-block presence byte at offset `19` |
| 20 | 1 | `property_block_presence` | `u8` | little | spec | a property-block presence byte at offset `20` |
| 21 | 4 | `bounding_box_index` | `u32` | little | spec | a bounding-box index at offset `21` |
| 25 | 24 | `position` | `f64[3]` | little | spec | The position triple is at offset `25` |
| 49 | 24 | `direction` | `f64[3]` | little | spec | the model-space direction triple at offset `49` |
| 73 | 16 | `point_parameters` | `f64[2]` | little | spec | the two construction parameters at offset `73` |
| 89 | 4 | `reference_type` | `u32` | little | spec | `refType` at offset `89` |
| 93 | 1 | `tangent_prefix` | `u8` | little | spec | a tangent-data prefix byte at offset `93` |
| 94 | 24 | `tangent_point_data` | `f64[3]` | little | spec | a tangent triple at offset `94` |
| 118 | 4 | `input_count` | `u32` | little | spec | the counted input-reference count at offset `118` |

## `design_hole_direct_selection_prefix`

Spec §3.1 · layout: byte offsets · size: 40 B

Fixed prefix through the variable asset UUID. The context UUID and nested indexed records follow.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The direct support-face selection has type GUID |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes at offsets `11` through `20` |
| 21 | 1 | `nested_selection_marker` | `u8` | little | spec | `u8 1` at offset `21` · value `1` |
| 22 | 4 | `nested_record_index` | `u32` | little | spec | nested record index at offset `22` |
| 26 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes at offsets `26` through `31` |
| 32 | 4 | `asset_presence` | `u32` | little | spec | `u32 1` at offset `32` · value `1` |
| 36 | 4 | `asset_uuid_code_unit_count` | `u32` | little | spec | the asset UUID code-unit count at offset `36` |

## `scale_modern_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 79 B

Offsets are relative to the modern Scale scope's primary indexed header. The ordered-reference tail continues after this fixed operation prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | scope has a 303-byte token-independent frame |
| 20 | 4 | `factor_kind` | `u32` | little | spec | Primary-header offset 20 stores u32 factor kind `1` |
| 25 | 8 | `factor` | `f64` | little | spec | the positive finite f64 at offset 25 is the uniform scale factor |
| 33 | 11 | `center_reference` | `bytes[11]` | little | spec | The marked references at offsets 33 and 44 name the fifth ordered reference |
| 44 | 11 | `factor_reference` | `bytes[11]` | little | spec | The marked references at offsets 33 and 44 name the fifth ordered reference and the first ordered reference |
| 55 | 4 | `factor_tail_one` | `u32` | little | spec | Offset 55 stores u32 `1` |
| 60 | 4 | `body_group_one` | `u32` | little | spec | offsets 60 and 64 each store u32 `1` |
| 64 | 4 | `body_group_kind` | `u32` | little | spec | offsets 60 and 64 each store u32 `1` |
| 68 | 11 | `body_group_marker` | `bytes[11]` | little | spec | the marked reference at offset 68 names the second ordered reference |

Unstated regions:

- `11..20` (9 B): The modern fixed operation fields begin at primary-header offset 20.
- `24..25` (1 B): Modern primary-header byte 24 is zero.
- `59..60` (1 B): Modern primary-header byte 59 is zero.

## `scale_legacy_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 75 B

Offsets are relative to the legacy Scale scope's primary indexed header. The frame tail carries the ordered-reference members.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | legacy English `Scale` scope has a 307-byte frame |
| 21 | 8 | `factor` | `f64` | little | spec | the positive finite f64 uniform factor at offset 21 |
| 29 | 11 | `center_reference` | `bytes[11]` | little | spec | marked references at offsets 29, 40, and 64 naming the final |
| 40 | 11 | `factor_reference` | `bytes[11]` | little | spec | marked references at offsets 29, 40, and 64 naming the final, first, and second ordered references |
| 51 | 4 | `factor_kind` | `u32` | little | spec | Offset 51 stores u32 `1` |
| 55 | 1 | `zero_byte` | `u8` | little | spec | byte 55 is zero |
| 56 | 4 | `tail_one` | `u32` | little | spec | offsets 56 and 60 each store u32 `1` |
| 60 | 4 | `body_group_one` | `u32` | little | spec | offsets 56 and 60 each store u32 `1` |
| 64 | 11 | `body_group_reference` | `bytes[11]` | little | spec | marked references at offsets 29, 40, and 64 naming the final, first, and second ordered references |

Unstated regions:

- `11..16` (5 B): The legacy fixed operation prefix retains five bytes before the zero run at offsets 16 through 20.
- `16..21` (5 B): Primary-header offsets 16 through 20 are zero.

## `design_body_map_prefix_10`

Spec §3.1 · layout: byte offsets · size: 25 B

Ten-reserved-byte variant. Offsets are relative to the typed body-map indexed header. The first selector/entity pair starts at offset 25.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | eleven-byte indexed header |
| 11 | 10 | `reserved_zero_run` | `bytes[10]` | little | spec | either ten or eleven reserved zero bytes |
| 21 | 4 | `pair_count` | `u32` | little | spec | and a `u32 count` |

Cross-checked against code:

- `crates/cadmpeg-codec-f3d/src/design/body.rs` — The body-map decoder accepts both reserved-zero variants.

## `design_body_map_prefix_11`

Spec §3.1 · layout: byte offsets · size: 26 B

Eleven-reserved-byte variant. Offsets are relative to the typed body-map indexed header. The first selector/entity pair starts at offset 26.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | eleven-byte indexed header |
| 11 | 11 | `reserved_zero_run` | `bytes[11]` | little | spec | either ten or eleven reserved zero bytes |
| 22 | 4 | `pair_count` | `u32` | little | spec | and a `u32 count` |

## `paramesh_entry_name_prefix`

Spec §3.1 · layout: byte offsets · size: 32 B

Offsets are relative to the entry-name record's indexed header. The variable u32-count UTF-16LE entry name starts at offset 32 and ends at the primary-record boundary.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | followed by ten zero bytes |
| 21 | 11 | `guid_record_reference` | `bytes[11]` | little | spec | a marked same-segment reference to the GUID record at offset 21 |

## `paramesh_guid_join_prefix`

Spec §3.1 · layout: byte offsets · size: 83 B

Offsets are relative to the container-GUID record's indexed header. A type-specific tail can follow the fixed join prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by 21 zero bytes |
| 11 | 21 | `zero_run_21` | `bytes[21]` | little | spec | followed by 21 zero bytes |
| 32 | 40 | `fusion_uuid` | `bytes[40]` | little | spec | the 36-byte LP-ASCII `fusion_uuid` at offset 32 |
| 72 | 11 | `entry_name_backlink` | `bytes[11]` | little | spec | a marked same-segment backlink to the entry-name record at offset 72 |

## `paramesh_mesh_body_join_prefix`

Spec §3.1 · layout: byte offsets · size: 564 B

Offsets are relative to the mesh-body primary indexed header. Presentation fields occupy the unstated spans, and the primary record can continue after this fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by ten zero bytes. |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Its indexed header is followed by ten zero bytes. |
| 42 | 128 | `first_transform` | `f64[16]` | little | spec | two equal row-major 4×4 f64 affine matrices at record-relative offsets 42 and 171 |
| 171 | 128 | `second_transform` | `f64[16]` | little | spec | two equal row-major 4×4 f64 affine matrices at record-relative offsets 42 and 171 |
| 508 | 11 | `feature_scope_reference` | `bytes[11]` | little | spec | Marked same-segment references at offsets 508, 519, 530, 541, and 553 name the owning `Base Mesh Feature` scope, the reciprocal body wrapper, a `Body` owner, the GUID record, and a Scene node. |
| 519 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | Marked same-segment references at offsets 508, 519, 530, 541, and 553 name the owning `Base Mesh Feature` scope, the reciprocal body wrapper, a `Body` owner, the GUID record, and a Scene node. |
| 530 | 11 | `body_owner_reference` | `bytes[11]` | little | spec | Marked same-segment references at offsets 508, 519, 530, 541, and 553 name the owning `Base Mesh Feature` scope, the reciprocal body wrapper, a `Body` owner, the GUID record, and a Scene node. |
| 541 | 11 | `container_guid_reference` | `bytes[11]` | little | spec | Marked same-segment references at offsets 508, 519, 530, 541, and 553 name the owning `Base Mesh Feature` scope, the reciprocal body wrapper, a `Body` owner, the GUID record, and a Scene node. |
| 553 | 11 | `scene_node_reference` | `bytes[11]` | little | spec | Marked same-segment references at offsets 508, 519, 530, 541, and 553 name the owning `Base Mesh Feature` scope, the reciprocal body wrapper, a `Body` owner, the GUID record, and a Scene node. |

Unstated regions:

- `21..42` (21 B): The remaining mesh-body prologue is outside this fixed join table.
- `170..171` (1 B): One structural byte separates the two transform blocks.
- `299..508` (209 B): Presentation fields occupy the span before the feature-scope reference.
- `552..553` (1 B): One structural byte separates the GUID and Scene-node references.

## `paramesh_mesh_collection_prefix`

Spec §3.1 · layout: byte offsets · size: 38 B

Offsets are relative to the mesh-collection indexed header. The nested CommonData record starts at the end of this prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | followed by ten zero bytes, a u32 body count at offset 21 |
| 21 | 4 | `body_count` | `u32` | little | spec | a u32 body count at offset 21 |
| 25 | 2 | `constant_01_01` | `bytes[2]` | little | spec | bytes `01 01` at offset 25 |
| 27 | 11 | `texture_table_reference` | `bytes[11]` | little | spec | a marked same-segment texture-table reference at offset 27 |

## `paramesh_mesh_collection_base_prefix`

Spec §3.1 · layout: byte offsets · size: 24 B

Offsets are relative to the nested CommonData indexed header at collection offset 38. The variable body-reference list starts at nested offset 24, which is collection offset 62.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by nine zero bytes |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | followed by nine zero bytes |
| 20 | 4 | `body_count` | `u32` | little | spec | a second u32 body count at collection offset 58 |

## `paramesh_texture_table_prefix`

Spec §3.1 · layout: byte offsets · size: 25 B

Offsets are relative to the texture-table indexed header. The first variable flags-map entry starts at offset 25.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | followed by ten zero bytes and a u32 flags-map count |
| 21 | 4 | `flags_map_count` | `u32` | little | spec | a u32 flags-map count at offset 21 |

## `paramesh_texture_filename_prefix`

Spec §3.1 · layout: byte offsets · size: 25 B

Offsets are relative to the filename-record indexed header. UTF-16LE code units start at offset 25 and continue to the primary-record boundary.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/mesh.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | followed by ten zero bytes and one nonempty u32-count UTF-16LE archive-entry basename |
| 21 | 4 | `basename_code_unit_count` | `u32` | little | spec | one nonempty u32-count UTF-16LE archive-entry basename |

## `paramesh_body_wrapper`

Spec §3.1 · layout: byte offsets · size: 40 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | its indexed header, ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes |
| 21 | 11 | `body_reference` | `bytes[11]` | little | spec | a marked same-segment body reference at offset 21 |
| 32 | 8 | `zero_tail_8` | `bytes[8]` | little | spec | and eight zero bytes |

## `paramesh_feature_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 25 B

Offsets are relative to the `Base Mesh Feature` indexed header. The ordered body-reference list starts at offset 25.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | followed by ten zero bytes, a u32 body count at offset 21 |
| 21 | 4 | `body_count` | `u32` | little | spec | a u32 body count at offset 21 |

## `paramesh_feature_scope_base`

Spec §3.1 · layout: byte offsets · size: 30 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/mesh.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | its indexed header, eight zero bytes |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | eight zero bytes |
| 19 | 11 | `scope_owner_reference` | `bytes[11]` | little | spec | a marked same-segment owner reference at offset 19 |

## `paramesh_scene_state`

Spec §3.1 · layout: byte offsets · size: 95 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | its indexed header, 34 zero bytes |
| 11 | 34 | `zero_run_34` | `bytes[34]` | little | spec | 34 zero bytes |
| 45 | 1 | `footer_marker` | `u8` | little | spec | byte `01` |
| 46 | 49 | `footer_mask` | `bytes[49]` | little | spec | and a 49-byte mask |

## `paramesh_scene_node`

Spec §3.1 · layout: byte offsets · size: 133 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by 14 zero bytes |
| 11 | 14 | `zero_run_14` | `bytes[14]` | little | spec | followed by 14 zero bytes |
| 25 | 4 | `constant_two_a` | `u32` | little | spec | u32 values `2` at offsets 25 and 29 |
| 29 | 4 | `constant_two_b` | `u32` | little | spec | u32 values `2` at offsets 25 and 29 |
| 33 | 11 | `scene_state_reference` | `bytes[11]` | little | spec | a marked same-segment Scene-state reference at offset 33 |
| 44 | 4 | `constant_three` | `u32` | little | spec | u32 value `3` at offset 44 |
| 48 | 11 | `auxiliary_record_reference` | `bytes[11]` | little | spec | a marked same-segment auxiliary-record reference at offset 48 |
| 59 | 24 | `zero_run_24` | `bytes[24]` | little | spec | 24 zero bytes at offset 59 |
| 83 | 1 | `footer_marker` | `u8` | little | spec | the same 50-byte Scene footer at offset 83 |
| 84 | 49 | `footer_mask` | `bytes[49]` | little | spec | the same 50-byte Scene footer at offset 83 |

## `paramesh_collection_owner_backlink_prefix`

Spec §3.1 · layout: byte offsets · size: 273 B

Offsets are relative to the collection-owner indexed header. The record can continue after this fixed backlink prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The owner has type GUID |
| 262 | 11 | `collection_backlink` | `bytes[11]` | little | spec | Its marked same-segment reference at offset 262 points back to the collection. |

Unstated regions:

- `11..262` (251 B): The owner payload before the reciprocal collection reference is outside this table.

## `assembly_operand_path_locator_reference_run`

Spec §Assembly operands · layout: byte offsets · size: 26 B

Offsets are relative to the count. The run starts at scope offset 47 in the 399-byte As-built form, offset 362 in the 627-, 637-, and 692-byte forms, and offset 358 in the 633- and 732-byte forms.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `locator_count` | `u32` | little | spec | stores a u32 count of two |
| 4 | 11 | `first_locator_reference` | `bytes[11]` | little | spec | followed by two marked same-segment operand-path-locator references |
| 15 | 11 | `second_locator_reference` | `bytes[11]` | little | spec | followed by two marked same-segment operand-path-locator references |

Cross-checked against code:

- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The As-built form uses the two tabulated scope-relative locator-reference offsets.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The standard assembly forms use the two tabulated scope-relative locator-reference offsets.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The compact assembly forms use the two tabulated scope-relative locator-reference offsets.

## `assembly_as_built_421_scope`

Spec §Assembly operands · layout: byte offsets · size: 421 B

Offsets are relative to the primary indexed header. The reference table starts with its marked entry at offset 189; each entry is 11 bytes. All four admitted 421-byte As-built generations use this layout.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | In a 421-byte `As-built` scope |
| 185 | 4 | `reference_count` | `u32` | little | spec | Offset 185 stores u32 value 11 · value `11` |
| 189 | 121 | `reference_entries` | `bytes[121]` | little | spec | Entries 0 and 1 form the first operand carrier pair. Entries 2 and 3 form the second pair. Entries 4 through 7 are the placement owners in OffsetX, OffsetY, OffsetZ, and AngleZ order. Entry 8 is the solved connector-frame carrier. Entries 9 and 10 are the generation-specific degree-of-freedom limit owners. |
| 310 | 4 | `reference_trailer` | `bytes[4]` | little | spec | Offset 310 stores four `ff` bytes · value `[255, 255, 255, 255]` |
| 314 | 4 | `kind_length` | `u32` | little | spec | offset 314 stores u32 value 8 · value `8` |
| 318 | 16 | `kind` | `bytes[16]` | little | spec | offset 318 stores the UTF-16LE string `As-built` |
| 334 | 4 | `feature_ordinal` | `u32` | little | spec | offset 334 stores the feature ordinal |

Unstated regions:

- `11..185` (174 B): The fixed scope prologue before the reference count is not assigned.
- `338..421` (83 B): The fixed frame tail before the paired indexed header is not assigned.

## `assembly_as_built_421_frame_376`

Spec §Assembly operands · layout: byte offsets · size: 389 B

Offsets are relative to the frame carrier's class-376 primary indexed header. The paired indexed header is class 272 at offset 389 and repeats the frame record index.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | \| `364 / 272` \| `293 / 272` \| |
| 45 | 4 | `matrix_prefix` | `bytes[4]` | little | spec | \| `364 / 272` \| `293 / 272` \| · value `[1, 1, 0, 0]` |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | \| `364 / 272` \| `293 / 272` \| |

Unstated regions:

- `11..45` (34 B): The frame-carrier prologue before its matrix marker is not assigned.
- `177..389` (212 B): The frame-carrier tail before the paired indexed header is not assigned.

## `assembly_as_built_421_frame_327`

Spec §Assembly operands · layout: byte offsets · size: 390 B

Offsets are relative to the frame carrier's class-327 primary indexed header. The paired indexed header is class 262 at offset 390 and repeats the frame record index.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | \| `420 / 262` \| `378 / 262` \| |
| 46 | 4 | `matrix_prefix` | `bytes[4]` | little | spec | \| `420 / 262` \| `378 / 262` \| · value `[1, 1, 0, 0]` |
| 50 | 128 | `matrix` | `f64[16]` | little | spec | \| `420 / 262` \| `378 / 262` \| |

Unstated regions:

- `11..46` (35 B): The frame-carrier prologue before its matrix marker is not assigned.
- `178..390` (212 B): The frame-carrier tail before the paired indexed header is not assigned.

## `assembly_as_built_421_frame_448`

Spec §Assembly operands · layout: byte offsets · size: 390 B

Offsets are relative to the frame carrier's class-448 primary indexed header. The paired indexed header is class 263 at offset 390 and repeats the frame record index.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | \| `417 / 263` \| `318 / 263` \| |
| 46 | 4 | `matrix_prefix` | `bytes[4]` | little | spec | \| `417 / 263` \| `318 / 263` \| · value `[1, 1, 0, 0]` |
| 50 | 128 | `matrix` | `f64[16]` | little | spec | \| `417 / 263` \| `318 / 263` \| |

Unstated regions:

- `11..46` (35 B): The frame-carrier prologue before its matrix marker is not assigned.
- `178..390` (212 B): The frame-carrier tail before the paired indexed header is not assigned.

## `assembly_as_built_421_frame_297`

Spec §Assembly operands · layout: byte offsets · size: 385 B

Offsets are relative to the frame carrier's class-297 primary indexed header. The paired indexed header is class 258 at offset 385 and repeats the frame record index.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | \| `457 / 258` \| `418 / 258` \| |
| 45 | 4 | `matrix_prefix` | `bytes[4]` | little | spec | \| `457 / 258` \| `418 / 258` \| · value `[1, 1, 0, 0]` |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | \| `457 / 258` \| `418 / 258` \| |

Unstated regions:

- `11..45` (34 B): The frame-carrier prologue before its matrix marker is not assigned.
- `177..385` (208 B): The frame-carrier tail before the paired indexed header is not assigned.

## `assembly_operand_path_locator`

Spec §Assembly operands · layout: byte offsets · size: 190 B

Offsets are relative to the locator's indexed header. The variable-length occurrence-path record starts immediately after this record.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte header is followed by ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Its eleven-byte header is followed by ten zero bytes |
| 21 | 11 | `nonzero_record_reference` | `bytes[11]` | little | spec | a nonzero marked same-segment reference at offset 21 |
| 32 | 1 | `zero_32` | `u8` | little | spec | one zero byte at offset 32 |
| 33 | 128 | `transform` | `f64[16]` | little | spec | a row-major rigid 4×4 transform at offset 33 |
| 161 | 1 | `zero_161` | `u8` | little | spec | one zero byte at offset 161 |
| 162 | 11 | `scope_backlink` | `bytes[11]` | little | spec | a marked backlink to the assembly-operation scope at offset 162 |
| 173 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | a marked reference to record index `N+2` at offset 173 |
| 184 | 4 | `constant_two` | `u32` | little | spec | u32 value 2 at offset 184 |
| 188 | 2 | `zero_tail_2` | `bytes[2]` | little | spec | two zero bytes at offset 188 |

## `assembly_operand_path_wrapper`

Spec §Assembly operands · layout: byte offsets · size: 37 B

Offsets are relative to the wrapper's indexed header. The next indexed record starts at the end of this fixed record.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | its eleven-byte indexed header, ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | its eleven-byte indexed header, ten zero bytes |
| 21 | 1 | `constant_one_byte` | `u8` | little | spec | byte value 1 at offset 21 |
| 22 | 4 | `constant_one_word` | `u32` | little | spec | u32 value 1 at offset 22 |
| 26 | 11 | `path_reference` | `bytes[11]` | little | spec | a marked reference to path record `N+1` at offset 26 |

## `assembly_axial_construction_carrier`

Spec §Assembly operands · layout: byte offsets · size: 391 B

Offsets are relative to the construction carrier's primary indexed header. The uninterpreted gaps retain bytes outside the settled transform and axis-reference fields.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `primary_indexed_header` | `bytes[11]` | little | spec | The fixed span from the primary header through the paired header is 391 bytes. |
| 48 | 128 | `operand_transform` | `f64[16]` | little | spec | The carrier's row-major rigid transform starts at offset 48 |
| 192 | 11 | `first_axis_record_reference` | `bytes[11]` | little | spec | Marked same-segment axis-record references start at offsets 192 and 208. |
| 208 | 11 | `second_axis_record_reference` | `bytes[11]` | little | spec | Marked same-segment axis-record references start at offsets 192 and 208. |
| 380 | 11 | `paired_indexed_header` | `bytes[11]` | little | spec | The carrier's paired indexed header starts at offset 380 |

Unstated regions:

- `11..48` (37 B): The construction payload before the rigid transform is not assigned.
- `176..192` (16 B): The bytes between the transform and first axis reference are not assigned.
- `203..208` (5 B): Five bytes separate the two axis references.
- `219..380` (161 B): The remaining construction payload before the paired header is not assigned.

## `assembly_axial_selector_prefix`

Spec §Assembly operands · layout: byte offsets · size: 37 B

Offsets are relative to the selector record's primary indexed header. The two variable LP-UTF16 selector GUIDs follow this prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | its eleven-byte indexed header, eleven zero bytes |
| 11 | 11 | `zero_run_11` | `bytes[11]` | little | spec | its eleven-byte indexed header, eleven zero bytes |
| 22 | 11 | `nested_record_reference` | `bytes[11]` | little | spec | a marked same-segment reference to the selector-record index plus three |
| 33 | 4 | `constant_one` | `u32` | little | spec | and u32 value 1. Two 36-code-unit LP-UTF16 GUIDs follow |

## `assembly_axial_role_prefix`

Spec §Assembly operands · layout: byte offsets · size: 29 B

Offsets are relative to the role record's indexed header. The 36-code-unit UTF-16 role payload follows this fixed prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | starts with its eleven-byte indexed header, ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | starts with its eleven-byte indexed header, ten zero bytes |
| 21 | 4 | `constant_one` | `u32` | little | spec | ten zero bytes, u32 value 1 |
| 25 | 4 | `role_code_unit_count` | `u32` | little | spec | the u32 code-unit count of a 36-code-unit LP-UTF16 role GUID |

## `grouped_recipe_reference_prefix`

Spec §3.1 · layout: byte offsets · size: 18 B

Offsets are relative to the recipe prefix. Five variable-length counted operand groups and one final u32 zero follow this fixed header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 10 | `zero_run_10` | `bytes[10]` | little | spec | stores ten zero bytes |
| 10 | 4 | `constant_one` | `u32` | little | spec | `u32 1` |
| 14 | 4 | `group_count` | `u32` | little | spec | `u32 5`, and exactly five groups |

## `combine_standard_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 33 B

Offsets are relative to the primary indexed scope header. The variable scope body follows this fixed prologue.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | nine zero bytes at offsets 11 through 19 |
| 20 | 4 | `operation` | `u32` | little | spec | the Boolean operation u32 at offset 20 |
| 24 | 1 | `zero_flag` | `u8` | little | spec | zero at byte 24 |
| 25 | 1 | `keep_tools` | `u8` | little | spec | the keep-tools Boolean at offset 25 |
| 26 | 7 | `zero_run_7` | `bytes[7]` | little | spec | seven zero bytes at offsets 26 through 32 |

## `combine_compact_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 46 B

Offsets are relative to the class-387 primary indexed scope header. The variable scope body follows this fixed prologue.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes at offsets 11 through 20 |
| 21 | 4 | `operation` | `u32` | little | spec | the operation at offset 21 |
| 25 | 1 | `keep_tools` | `u8` | little | spec | the keep-tools Boolean at offset 25 |
| 26 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes |
| 29 | 2 | `reference_form` | `bytes[2]` | little | spec | `01 00` |
| 31 | 4 | `constant_one` | `u32` | little | spec | u32 one at offset 31 |
| 35 | 1 | `reference_marker` | `u8` | little | spec | a marked nonzero same-segment u64 reference at offset 35 |
| 36 | 8 | `reference_value` | `u64` | little | spec | a marked nonzero same-segment u64 reference at offset 35 |
| 44 | 2 | `reference_tail` | `bytes[2]` | little | spec | a two-byte zero trailing field |

## `combine_extended_reference_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 46 B

Offsets are relative to the class-329 primary indexed scope header. The variable scope body follows this fixed prologue.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 18 | `zero_run_18` | `bytes[18]` | little | spec | eighteen zero bytes at offsets 11 through 28 |
| 29 | 1 | `form_marker` | `u8` | little | spec | byte one at offset 29 |
| 30 | 1 | `keep_tools` | `u8` | little | spec | the keep-tools Boolean at offset 30 |
| 31 | 4 | `operation` | `u32` | little | spec | the operation at offset 31 |
| 35 | 1 | `reference_marker` | `u8` | little | spec | a marked nonzero same-segment u64 reference at offset 35 |
| 36 | 8 | `reference_value` | `u64` | little | spec | a marked nonzero same-segment u64 reference at offset 35 |
| 44 | 2 | `reference_tail` | `bytes[2]` | little | spec | a two-byte zero trailing field |

## `combine_external_selector_prefix`

Spec §3.1 · layout: byte offsets · size: 40 B

Offsets are relative to the tool body-selection header. The variable LP-UTF16 selector asset GUID starts at offset 40.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 14 | `zero_run_14` | `bytes[14]` | little | spec | followed by fourteen zero bytes |
| 25 | 1 | `nested_reference_marker` | `u8` | little | spec | a same-segment reference to `N+3` |
| 26 | 8 | `nested_record_index` | `u64` | little | spec | a same-segment reference to `N+3` |
| 34 | 2 | `nested_reference_tail` | `bytes[2]` | little | spec | a same-segment reference to `N+3` |
| 36 | 4 | `constant_one` | `u32` | little | spec | a same-segment reference to `N+3`, u32 one |

## `combine_external_selector_tail`

Spec §3.1 · layout: byte offsets · size: 62 B

Offsets are relative to the first byte after the cross-document reference. The same-index paired header starts at offset 62.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `constant_nine` | `u32` | little | spec | The selector tail is u32 `9` |
| 4 | 2 | `constant_two` | `u16` | little | spec | u16 `2` |
| 6 | 8 | `tail_value_0` | `u64` | little | spec | a retained u64 value |
| 14 | 4 | `constant_forty_eight` | `u32` | little | spec | u32 `48` |
| 18 | 8 | `tail_value_1` | `u64` | little | spec | a second retained u64 value |
| 26 | 11 | `nested_two_reference` | `bytes[11]` | little | spec | a same-segment reference to `N+2` |
| 37 | 2 | `zero_run_2` | `bytes[2]` | little | spec | two zero bytes |
| 39 | 11 | `nested_one_reference` | `bytes[11]` | little | spec | a same-segment reference to `N+1` |
| 50 | 1 | `zero_flag` | `u8` | little | spec | one zero byte |
| 51 | 11 | `scope_reference` | `bytes[11]` | little | spec | a same-segment reference to the owning scope |

## `indexed_companion_record_prefix`

Spec §3.1 · layout: byte offsets · size: 58 B

The stated field list tiles the stated 58-byte total exactly. The timestamp is a wall-clock value recorded at a parameter authoring event.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte indexed header |
| 11 | 20 | `zero_run_20` | `bytes[20]` | little | spec | is followed by 20 zero bytes |
| 31 | 1 | `owner_marker` | `u8` | little | spec | `01 + u32 owner_record_index` |
| 32 | 4 | `owner_record_index` | `u32` | little | spec | `01 + u32 owner_record_index` |
| 36 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes |
| 42 | 8 | `timestamp_micros` | `u64` | little | spec | a nonzero u64 Unix-epoch timestamp in microseconds |
| 50 | 8 | `zero_run_8` | `bytes[8]` | little | spec | and eight zero bytes |

## `named_solid_primitive_prologue`

Spec §3.1 · layout: byte offsets · size: 26 B

Offsets are relative to the primary indexed scope header. The ordered parameter-owner references and the paired header follow this fixed prologue.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | Bytes 11 through 19 are zero |
| 20 | 4 | `operation` | `u32` | little | spec | the result-operation u32 is at primary-header offset 20 |
| 24 | 1 | `zero_flag` | `u8` | little | spec | byte 24 is zero |
| 25 | 1 | `form_marker` | `u8` | little | spec | byte 25 is `0x01` |

## `shifted_cylinder_primitive_352_frame`

Spec §3.1 · layout: byte offsets · size: 352 B

Offsets are relative to the primary indexed scope header. The generic scope reference table begins at offset 174; the paired header follows the complete scope frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its bytes 11 through 20 are zero |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Its bytes 11 through 20 are zero |
| 21 | 1 | `form_marker` | `u8` | little | spec | byte 21 is `0x01` · value `1` |
| 22 | 4 | `operation` | `u32` | little | spec | the result-operation u32 is at offset 22 |
| 26 | 11 | `first_reference` | `bytes[11]` | little | spec | This first reference has a nested marker byte; the remaining three references are ordinary eleven-byte marked references. |
| 37 | 1 | `reference_gap` | `bytes[1]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 38 | 11 | `second_reference` | `bytes[11]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 49 | 11 | `third_reference` | `bytes[11]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 60 | 11 | `fourth_reference` | `bytes[11]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 71 | 1 | `compact_tail_marker` | `u8` | little | spec | The 352-byte form stores byte `0x01` at offset 71 |
| 72 | 4 | `compact_tail_count` | `u32` | little | spec | The 352-byte form stores byte `0x01` at offset 71, u32 `1` at offset 72 · value `1` |
| 76 | 11 | `compact_tail_reference` | `bytes[11]` | little | spec | one marked reference at offset 76 |
| 87 | 8 | `compact_tail_zero_run_8` | `bytes[8]` | little | spec | eight zero bytes at offsets 87 through 94 |
| 95 | 4 | `guid_code_unit_count` | `u32` | little | spec | a 36-code-unit LP-UTF16 GUID at offset 95 · value `36` |
| 99 | 72 | `guid` | `bytes[72]` | little | spec | a 36-code-unit LP-UTF16 GUID at offset 95 |
| 171 | 3 | `zero_run_3_after_guid` | `bytes[3]` | little | spec | Its generic scope reference table begins at offset 174 |
| 174 | 4 | `reference_count` | `u32` | little | spec | At offsets 174 and 302 respectively, the generic suffix stores the reference count |
| 178 | 55 | `references` | `bytes[55]` | little | spec | the ordered eleven-byte reference run |
| 233 | 4 | `history_state_id` | `u32` | little | spec | the current history state |
| 237 | 4 | `kind_code_unit_count` | `u32` | little | spec | the 17-code-unit `CylinderPrimitive` kind · value `17` |
| 241 | 34 | `kind` | `bytes[34]` | little | spec | the 17-code-unit `CylinderPrimitive` kind |
| 275 | 4 | `feature_ordinal` | `u32` | little | spec | the feature ordinal |
| 306 | 4 | `previous_history_state_id` | `u32` | little | spec | the previous history state at offsets 306 and 456 respectively |

Unstated regions:

- `279..306` (27 B): Scope-tail bytes between the feature ordinal and previous history state.
- `310..352` (42 B): Scope-tail bytes before the paired indexed header.

## `shifted_cylinder_primitive_502_frame`

Spec §3.1 · layout: byte offsets · size: 502 B

Offsets are relative to the primary indexed scope header. The generic scope reference table begins at offset 302; the paired header follows the complete scope frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its bytes 11 through 20 are zero |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Its bytes 11 through 20 are zero |
| 21 | 1 | `form_marker` | `u8` | little | spec | byte 21 is `0x01` · value `1` |
| 22 | 4 | `operation` | `u32` | little | spec | the result-operation u32 is at offset 22 |
| 26 | 11 | `first_reference` | `bytes[11]` | little | spec | This first reference has a nested marker byte; the remaining three references are ordinary eleven-byte marked references. |
| 37 | 1 | `reference_gap` | `bytes[1]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 38 | 11 | `second_reference` | `bytes[11]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 49 | 11 | `third_reference` | `bytes[11]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 60 | 11 | `fourth_reference` | `bytes[11]` | little | spec | Ordinary eleven-byte marked references begin at offsets 38, 49, and 60 |
| 71 | 1 | `zero_before_matrix` | `bytes[1]` | little | spec | The 502-byte form stores a row-major rigid 4x4 f64 frame at offset 72 |
| 72 | 128 | `matrix` | `f64[16]` | little | spec | The 502-byte form stores a row-major rigid 4x4 f64 frame at offset 72 |
| 200 | 8 | `zero_run_8_after_matrix` | `bytes[8]` | little | spec | eight zero bytes at offsets 200 through 207 |
| 208 | 15 | `construction_reference` | `bytes[15]` | little | spec | a 15-byte construction-reference block at offset 208 |
| 223 | 4 | `guid_code_unit_count` | `u32` | little | spec | a 36-code-unit LP-UTF16 GUID at offset 223 · value `36` |
| 227 | 72 | `guid` | `bytes[72]` | little | spec | a 36-code-unit LP-UTF16 GUID at offset 223 |
| 299 | 3 | `zero_run_3_after_guid` | `bytes[3]` | little | spec | bytes 299 through 301 are zero |
| 302 | 4 | `reference_count` | `u32` | little | spec | At offsets 174 and 302 respectively, the generic suffix stores the reference count |
| 306 | 77 | `references` | `bytes[77]` | little | spec | the ordered eleven-byte reference run |
| 383 | 4 | `history_state_id` | `u32` | little | spec | the current history state |
| 387 | 4 | `kind_code_unit_count` | `u32` | little | spec | the 17-code-unit `CylinderPrimitive` kind · value `17` |
| 391 | 34 | `kind` | `bytes[34]` | little | spec | the 17-code-unit `CylinderPrimitive` kind |
| 425 | 4 | `feature_ordinal` | `u32` | little | spec | the feature ordinal |
| 456 | 4 | `previous_history_state_id` | `u32` | little | spec | the previous history state at offsets 306 and 456 respectively |

Unstated regions:

- `429..456` (27 B): Scope-tail bytes between the feature ordinal and previous history state.
- `460..502` (42 B): Scope-tail bytes before the paired indexed header.

## `compact_loft_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 45 B

Offsets are relative to the primary indexed header. The variable scope body follows this fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | after its indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | has ten zero bytes after its indexed header |
| 21 | 4 | `one_run_4` | `bytes[4]` | little | spec | four bytes of value `1` |
| 25 | 4 | `operation` | `u32` | little | spec | the result-operation u32 at primary-header offset 25 |
| 29 | 1 | `zero_flag` | `u8` | little | spec | Byte 29 is zero |
| 30 | 4 | `all_ones` | `bytes[4]` | little | spec | offsets 30 through 33 are `ff ff ff ff` |
| 34 | 11 | `zero_run_11` | `bytes[11]` | little | spec | offsets 34 through 44 are zero |

## `fixed_pipe_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 31 B

Offsets are relative to the primary indexed header. The variable scope reference table and operand records follow this fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | the primary indexed header |
| 25 | 4 | `operation` | `u32` | little | spec | Primary-header offset 25 stores u32 `4`, which selects a new-body result |
| 29 | 1 | `section_shape` | `u8` | little | spec | Offset 29 value `1` selects a circular section |
| 30 | 1 | `filled` | `u8` | little | spec | offset 30 value `1` selects a filled section |

Unstated regions:

- `11..25` (14 B): The fixed Pipe prefix carries no admitted fields between its indexed header and the operation at offset 25.

## `legacy_pipe_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 32 B

Offsets are relative to the primary indexed header. The legacy scope reference table and variable operand records follow this prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | primary indexed header |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | offsets `11` through `19` are zero |
| 20 | 1 | `prefix_marker` | `u8` | little | spec | offset `20` is u8 `1` · value `1` |
| 21 | 5 | `zero_run_5` | `bytes[5]` | little | spec | offsets `21` through `25` are zero |
| 26 | 4 | `operation` | `u32` | little | spec | offset `26` is the result-operation u32 |
| 30 | 1 | `section_shape` | `u8` | little | spec | offset `30` is the section-shape u8 |
| 31 | 1 | `filled` | `u8` | little | spec | offset `31` is the filled-section u8 |

## `sketch_profile_region_selection_prefix`

Spec §3.1 · layout: byte offsets · size: 40 B

Offsets are relative to the N+3 selection header. The ordered variable-length region run starts at offset 40.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | starts with its eleven-byte indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes |
| 21 | 1 | `profile_reference_marker` | `u8` | little | spec | a marked reference to `N` |
| 22 | 4 | `profile_record_index` | `u32` | little | spec | a marked reference to `N` |
| 26 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes |
| 32 | 4 | `format_version` | `u32` | little | spec | u32 `1` |
| 36 | 4 | `region_count` | `u32` | little | spec | a nonzero u32 region count |

## `sketch_profile_region_member`

Spec §3.1 · layout: byte offsets · size: 40 B

The member repeats within each selected region. Region and member counts and the later-region marker are outside this fixed member frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `kind` | `u32` | little | spec | u32 kind `3` |
| 4 | 4 | `curve_primary_id` | `u32` | little | spec | one nonzero u32 primary persistent Sketch-curve identity |
| 8 | 12 | `zero_words_3` | `u32[3]` | little | spec | three zero u32 values |
| 20 | 12 | `incidence_words` | `u32[3]` | little | spec | three incidence values |
| 32 | 8 | `zero_words_2` | `u32[2]` | little | spec | two zero u32 values |

## `base_feature_class_377_prefix`

Spec §3.1 · layout: byte offsets · size: 363 B

Offsets are relative to the class-377 primary indexed header; the generic scope suffix and paired header remain part of the fixed frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | bytes 11 through 18 are zero |
| 19 | 1 | `body_reference_count_marker` | `u8` | little | spec | byte 19 is `0x01` · value `1` |
| 20 | 4 | `body_reference_count` | `u32` | little | spec | offset 20 stores u32 `2` · value `2` |
| 24 | 1 | `parameter_body_reference_marker` | `u8` | little | spec | Marked 15-byte elements at offsets 24 and 39 · value `1` |
| 25 | 4 | `parameter_body_record` | `u32` | little | spec | the PM body-reference record |
| 29 | 10 | `parameter_body_reference_field` | `bytes[10]` | little | spec | each element has a u32 value followed by ten zero bytes |
| 39 | 1 | `body_entity_reference_marker` | `u8` | little | spec | Marked 15-byte elements at offsets 24 and 39 · value `1` |
| 40 | 4 | `body_entity_suffix` | `u32` | little | spec | the Design body entity suffix |
| 44 | 10 | `body_entity_reference_field` | `bytes[10]` | little | spec | each element has a u32 value followed by ten zero bytes |
| 54 | 1 | `tag_body_based_on_faces_marker` | `u8` | little | spec | The property block at offset 54 · value `1` |
| 55 | 4 | `tag_body_based_on_faces_count` | `u32` | little | spec | has one entry named · value `1` |
| 59 | 4 | `tag_body_based_on_faces_key_length` | `u32` | little | spec | TagBodyBasedOnFaces · value `19` |
| 63 | 19 | `tag_body_based_on_faces_key` | `bytes[19]` | little | spec | TagBodyBasedOnFaces |
| 82 | 4 | `tag_body_based_on_faces_type_length` | `u32` | little | spec | IntrinsicMetaTypebool · value `21` |
| 86 | 21 | `tag_body_based_on_faces_type` | `bytes[21]` | little | spec | IntrinsicMetaTypebool |
| 107 | 2 | `tag_body_based_on_faces_value` | `u16` | little | spec | u16 value `1` · value `1` |
| 109 | 1 | `parameter_reference_group_marker` | `u8` | little | spec | The parameter-reference group at offset 109 · value `1` |
| 110 | 4 | `parameter_reference_group_count` | `u32` | little | spec | has count `1` · value `1` |
| 114 | 1 | `parameter_reference_marker` | `u8` | little | spec | repeats the PM body-reference record at offset 115 · value `1` |
| 115 | 4 | `parameter_reference_record` | `u32` | little | spec | repeats the PM body-reference record at offset 115 |
| 119 | 7 | `parameter_reference_field` | `bytes[7]` | little | spec | seven bytes after that member are zero |
| 126 | 1 | `scope_reference_member_marker` | `u8` | little | spec | The scope-reference member at offset 126 · value `1` |
| 127 | 4 | `scope_reference_member_record` | `u32` | little | spec | repeats the generic scope reference |
| 131 | 6 | `scope_reference_member_field` | `bytes[6]` | little | spec | six zero trailing bytes |
| 137 | 1 | `auxiliary_group_marker` | `u8` | little | spec | The auxiliary group starts at offset 137 · value `1` |
| 138 | 3 | `auxiliary_group_zero_run` | `bytes[3]` | little | spec | with three zero bytes |
| 141 | 1 | `auxiliary_reference_marker` | `u8` | little | spec | a marked auxiliary record at offset 141 · value `1` |
| 142 | 4 | `auxiliary_record` | `u32` | little | spec | a marked auxiliary record at offset 141 |
| 146 | 14 | `auxiliary_reference_field` | `bytes[14]` | little | spec | fourteen zero trailing bytes |
| 160 | 4 | `envelope_guid_code_unit_count` | `u32` | little | spec | A 36-code-unit LP-UTF16 envelope GUID starts at offset 160 · value `36` |
| 164 | 72 | `envelope_guid` | `bytes[72]` | little | spec | A 36-code-unit LP-UTF16 envelope GUID starts at offset 160 |
| 236 | 3 | `zero_run_3` | `bytes[3]` | little | spec | followed by three zero bytes at offsets 236 through 238 |
| 239 | 4 | `reference_count` | `u32` | little | spec | generic scope suffix starts with u32 `1` at offset 239 · value `1` |
| 243 | 1 | `generic_scope_reference_marker` | `u8` | little | spec | a marked copy of the scope reference at offset 243 · value `1` |
| 244 | 4 | `generic_scope_reference_record` | `u32` | little | spec | a marked copy of the scope reference at offset 243 |
| 248 | 6 | `generic_scope_reference_field` | `bytes[6]` | little | spec | six zero bytes, the current history-state identity |
| 254 | 4 | `history_state_id` | `u32` | little | spec | current history-state identity at offset 254 |
| 258 | 4 | `kind_length` | `u32` | little | spec | 12-code-unit `Base Feature` kind at offset 258 · value `12` |
| 262 | 24 | `kind` | `bytes[24]` | little | spec | 12-code-unit `Base Feature` kind at offset 258 |
| 286 | 4 | `feature_ordinal` | `u32` | little | spec | feature ordinal after its payload |
| 317 | 4 | `previous_history_state_id` | `u32` | little | spec | previous history-state identity follows the post-ordinal scope tail at offset 317 |

Unstated regions:

- `0..11` (11 B): The indexed header precedes the class-377 envelope payload.
- `290..317` (27 B): The post-ordinal scope tail precedes the previous history-state identity.
- `321..363` (42 B): The fixed class-377 tail closes the primary frame before its paired header.

## `base_feature_result_body_prefix`

Spec §3.1 · layout: byte offsets · size: 24 B

Offsets are relative to the primary indexed header. The two parallel 15-byte body-entry runs begin at offset 24.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | bytes 11 through 18 are zero |
| 19 | 1 | `body_count_marker` | `u8` | little | spec | byte 19 is `0x01` |
| 20 | 4 | `combined_body_reference_count` | `u32` | little | spec | offset 20 stores u32 `2N` |

## `base_feature_legacy_zero_body`

Spec §3.1 · layout: byte offsets · size: 55 B

Offsets are relative to the class-409 primary indexed header. The shared metadata record is the only retained body-snapshot reference.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | offsets 11 through 19 zero |
| 20 | 1 | `zero_body_marker` | `u8` | little | spec | offset 20 `u8 1` |
| 21 | 11 | `zero_run_11` | `bytes[11]` | little | spec | offsets 21 through 31 zero |
| 32 | 1 | `shared_metadata_marker` | `u8` | little | spec | offset 32 the shared-metadata marker |
| 33 | 8 | `shared_metadata_record` | `u64` | little | spec | offset 33 its u64 record index |
| 41 | 6 | `shared_metadata_field` | `bytes[6]` | little | spec | offset 41 the six-byte field |
| 47 | 8 | `zero_padding_8` | `bytes[8]` | little | spec | offsets 47 through 54 zero |

Unstated regions:

- `0..11` (11 B): The indexed header precedes the class-409 zero-body payload.

## `base_feature_legacy_444_zero_body`

Spec §3.1 · layout: byte offsets · size: 157 B

Offsets are relative to the class-444 primary indexed header. The fixed prefix ends at the 12-code-unit kind length; the generic scope tail and paired header follow it.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | nine zero bytes at offsets 11 through 19 |
| 20 | 1 | `zero_body_marker` | `u8` | little | spec | `u8 1` at offset 20 · value `1` |
| 21 | 11 | `zero_run_11` | `bytes[11]` | little | spec | eleven zero bytes at offsets 21 through 31 |
| 32 | 1 | `shared_metadata_marker` | `u8` | little | spec | `u8 1` at offset 32 · value `1` |
| 33 | 8 | `shared_metadata_record` | `u64` | little | spec | shared metadata u64 is at offset 33 |
| 41 | 14 | `shared_metadata_zero_tail` | `bytes[14]` | little | spec | bytes 41 through 54 are zero |
| 55 | 4 | `guid_code_unit_count` | `u32` | little | spec | A u32 36 at offset 55 · value `36` |
| 59 | 72 | `guid` | `bytes[72]` | little | spec | 36-code-unit GUID at offset 59 |
| 131 | 3 | `zero_run_3` | `bytes[3]` | little | spec | bytes 131 through 133 are zero |
| 134 | 4 | `reference_count` | `u32` | little | spec | u32 1 at offset 134 · value `1` |
| 138 | 1 | `scope_reference_marker` | `u8` | little | spec | `u8 1` at offset 138 · value `1` |
| 139 | 4 | `scope_reference_record` | `u32` | little | spec | repeated metadata u32 at offset 139 |
| 143 | 6 | `scope_reference_field` | `bytes[6]` | little | spec | six zero bytes at offsets 143 through 148 |
| 149 | 4 | `history_state_id` | `u32` | little | spec | current history-state u32 at offset 149 |
| 153 | 4 | `kind_length` | `u32` | little | spec | u32 12 at offset 153 · value `12` |

Unstated regions:

- `0..11` (11 B): The indexed header precedes the class-444 zero-body payload.

## `base_feature_result_body_entry`

Spec §3.1 · layout: byte offsets · size: 15 B

This layout repeats for each body entity suffix and each passive body-reference record, with entry bases at `24 + 15i` and `24 + 15N + 15i`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `reference_marker` | `u8` | little | spec | marked u64 Design body entity suffixes |
| 1 | 8 | `reference_value` | `u64` | little | spec | marked u64 Design body entity suffixes |
| 9 | 6 | `reference_field` | `bytes[6]` | little | spec | every value has a six-byte trailing field |

## `base_feature_compact_result_body_count`

Spec §3.1 · layout: byte offsets · size: 11 B

Offsets are relative to the byte after the two parallel 15-byte body-entry runs. The class-420 and class-452 compact forms use repeat marker 1; the class-444 form uses repeat marker 0.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `count_marker` | `u8` | little | spec | stores `0x01`, five zero bytes, `0x01`, and u32 `N` in the eleven-byte count field |
| 1 | 5 | `zero_run_5` | `bytes[5]` | little | spec | five zero bytes |
| 6 | 1 | `repeat_marker` | `u8` | little | spec | stores `0x01`, five zero bytes, `0x01`, and u32 `N` in the eleven-byte count field |
| 7 | 4 | `body_count` | `u32` | little | spec | and u32 `N` in the eleven-byte count field |

## `base_feature_compact_repeated_body_entry`

Spec §3.1 · layout: byte offsets · size: 11 B

This layout repeats for `N` entries immediately after the compact result-body count field.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `body_marker` | `u8` | little | spec | repeated u32 run contains the body entity suffixes |
| 1 | 4 | `body_entity_suffix` | `u32` | little | spec | repeated u32 run contains the body entity suffixes |
| 5 | 6 | `body_field` | `bytes[6]` | little | spec | six-byte trailing fields |

## `base_feature_compact_metadata_tail`

Spec §3.1 · layout: byte offsets · size: 16 B

Offsets are relative to the byte after the compact repeated body-entry run. The result-record run begins at offset 16.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `separator` | `u8` | little | spec | One zero byte and one marked u64 shared metadata-record index |
| 1 | 1 | `metadata_marker` | `u8` | little | spec | one marked u64 shared metadata-record index |
| 2 | 8 | `metadata_record` | `u64` | little | spec | shared metadata-record index |
| 10 | 2 | `metadata_field` | `bytes[2]` | little | spec | its shared metadata-record index has a two-byte trailing field |
| 12 | 4 | `result_count` | `u32` | little | spec | then u32 `N` |

## `base_feature_result_body_result_entry`

Spec §3.1 · layout: byte offsets · size: 11 B

This layout repeats for `N` result-record entries after the result count.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `result_marker` | `u8` | little | spec | marked u32 result-record indices |
| 1 | 4 | `result_record` | `u32` | little | spec | marked u32 result-record indices |
| 5 | 6 | `result_field` | `bytes[6]` | little | spec | six-byte trailing fields |

## `base_feature_body_snapshot_prefix`

Spec §3.1 · layout: byte offsets · size: 24 B

Offsets are relative to the primary indexed header. The repeated body-entry run begins at offset 24 and has 15 bytes per body.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | At offsets 11 through 18 it stores eight zero bytes |
| 19 | 1 | `body_count_marker` | `u8` | little | spec | then `u8 1` at offset 19 |
| 20 | 4 | `body_count` | `u32` | little | spec | and u32 `N` at offset 20 |

## `base_feature_body_snapshot_body_entry`

Spec §3.1 · layout: byte offsets · size: 15 B

This layout repeats once for each body, with the entry base at primary-scope offset `24 + 15i`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `body_marker` | `u8` | little | spec | `u8 1 + u64 body entity suffix + six-byte field` |
| 1 | 8 | `body_entity_suffix` | `u64` | little | spec | `u8 1 + u64 body entity suffix + six-byte field` |
| 9 | 6 | `body_entity_field` | `bytes[6]` | little | spec | `u8 1 + u64 body entity suffix + six-byte field` |

## `base_feature_body_snapshot_compact_preamble`

Spec §3.1 · layout: byte offsets · size: 8 B

The compact body-snapshot preamble occupies the eight bytes immediately after the repeated body-entry run.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `preamble` | `bytes[8]` | little | spec | the eight-byte sequence `01 00 00 00 01 00 00 00` |

## `base_feature_body_snapshot_expanded_preamble`

Spec §3.1 · layout: byte offsets · size: 9 B

The expanded body-snapshot preamble occupies the nine bytes immediately after the repeated body-entry run. Its final zero byte is also the first byte of the linkage tail.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 9 | `preamble` | `bytes[9]` | little | spec | the nine-byte sequence `01 00 00 00 00 01 00 00 00` |

## `base_feature_body_snapshot_linkage_tail`

Spec §3.1 · layout: byte offsets · size: 57 B

Offsets are relative to the linkage-tail anchor `A = 184 + 15N`. The third GUID starts at offset 57; in the expanded preamble, the second GUID ends at `A + 1` and shares its final zero byte with the tail at `A`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `zero_run_2` | `bytes[2]` | little | spec | two zero bytes |
| 2 | 5 | `envelope_prefix` | `bytes[5]` | little | spec | `01 01 00 00 00` |
| 7 | 1 | `first_body_marker` | `u8` | little | spec | a marked first-body suffix at offsets `A + 7` and `A + 8` |
| 8 | 8 | `first_body_entity_suffix` | `u64` | little | spec | a marked first-body suffix at offsets `A + 7` and `A + 8` |
| 16 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes |
| 19 | 1 | `linkage_marker` | `u8` | little | spec | a marked linkage record at `A + 19` and `A + 20` |
| 20 | 8 | `linkage_record` | `u64` | little | spec | a marked linkage record at `A + 19` and `A + 20` |
| 28 | 6 | `zero_run_6` | `bytes[6]` | little | spec | six zero bytes |
| 34 | 4 | `relation_count` | `u32` | little | spec | u32 `1` at `A + 34` |
| 38 | 1 | `auxiliary_marker` | `u8` | little | spec | a marked auxiliary record at `A + 38` and `A + 39` |
| 39 | 8 | `auxiliary_record` | `u64` | little | spec | a marked auxiliary record at `A + 38` and `A + 39` |
| 47 | 6 | `trailing_zero_run_6` | `bytes[6]` | little | spec | six zero bytes |
| 53 | 4 | `trailing_zero_run_4` | `bytes[4]` | little | spec | four zero bytes |

## `base_feature_body_snapshot_guid`

Spec §3.1 · layout: byte offsets · size: 76 B

Each GUID occupies a u32 code-unit count followed by 72 UTF-16LE payload bytes. The layout repeats for the two initial GUIDs and the third GUID after the linkage tail.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `code_unit_count` | `u32` | little | spec | LP-UTF16 GUIDs of 36 code units |
| 4 | 72 | `guid_utf16` | `bytes[72]` | little | spec | LP-UTF16 GUIDs of 36 code units |

## `base_feature_body_snapshot_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 26 B

Offsets are relative to `T = A + 133`, after the third GUID. The LP-UTF16 kind payload follows this prefix at offset 26.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `zero_run_3` | `bytes[3]` | little | spec | stores three zero bytes |
| 3 | 4 | `reference_count` | `u32` | little | spec | the one-member reference table at `T + 3` |
| 7 | 1 | `reference_marker` | `u8` | little | spec | the one-member reference table at `T + 3` |
| 8 | 4 | `reference_member` | `u32` | little | spec | the reference value is at `T + 8` |
| 12 | 6 | `reference_zero_run` | `bytes[6]` | little | spec | the one-member reference table at `T + 3` |
| 18 | 4 | `history_state_id` | `u32` | little | spec | the current history-state identity at `T + 18` |
| 22 | 4 | `kind_code_unit_count` | `u32` | little | spec | the LP-UTF16 scope kind at `T + 22` |

## `split_face_compact_prefix`

Spec §3.1 · layout: byte offsets · size: 32 B

Offsets are relative to the primary SplitFace indexed header. The first marked construction reference follows this prefix at offset 32.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 11 | `zero_run_11` | `bytes[11]` | little | spec | eleven zero bytes at offsets 11 through 21 |
| 22 | 1 | `first_marker` | `u8` | little | spec | byte `01` at offset 22 |
| 23 | 4 | `zero_run_4` | `bytes[4]` | little | spec | four zero bytes at offsets 23 through 26 |
| 27 | 2 | `marker_pair` | `bytes[2]` | little | spec | bytes `01 01` at offsets 27 and 28 |
| 29 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes at offsets 29 through 31 |

## `form_legacy_one_cage_owner`

Spec §1.1.1 · layout: byte offsets · size: 81 B

Offsets are relative to the primary indexed header. The owner/paired/nested class triples are 335/262/328, 395/264/329, 448/258/276, and 295/258/274.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | indexed header |
| 11 | 14 | `zero_run_14` | `bytes[14]` | little | spec | fourteen zero bytes |
| 25 | 1 | `owner_marker` | `u8` | little | spec | `u8 1`, the owning Form scope's u64 record index |
| 26 | 8 | `owner_scope_record_index` | `u64` | little | spec | owning Form scope's u64 record index |
| 34 | 24 | `zero_run_24` | `bytes[24]` | little | spec | twenty-four zero bytes |
| 58 | 1 | `nested_marker` | `u8` | little | spec | marked nested cage-object reference |
| 59 | 8 | `nested_record_index` | `u64` | little | spec | nested cage-object reference |
| 67 | 3 | `nested_zero_run` | `bytes[3]` | little | spec | three zero bytes |
| 70 | 1 | `owner_repeat_marker` | `u8` | little | spec | repeated marked owning-scope reference |
| 71 | 8 | `owner_repeat_scope` | `u64` | little | spec | repeated marked owning-scope reference |
| 79 | 2 | `tail_zero_run` | `bytes[2]` | little | spec | two zero bytes |

## `form_compact_one_cage_list`

Spec §1.1.1 · layout: byte offsets · size: 100 B

Offsets are relative to the primary indexed header. The class tags are dynamic; the frame length and fixed fields select the compact one-cage form.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Ten zero bytes follow the indexed header |
| 21 | 1 | `owner_marker` | `u8` | little | spec | `u8 1`, the owning Form scope's u64 record index |
| 22 | 8 | `owner_scope_record_index` | `u64` | little | spec | owning Form scope's u64 record index |
| 30 | 2 | `zero_run_2` | `bytes[2]` | little | spec | two zero bytes |
| 32 | 4 | `cage_count` | `u32` | little | spec | a u32 cage count of one |
| 36 | 1 | `member_marker` | `u8` | little | spec | The sole member is `u8 1` |
| 37 | 8 | `cage_object_record_index` | `u64` | little | spec | a u64 cage-object record index |
| 45 | 2 | `member_zero` | `u16` | little | spec | `u16 0` |
| 47 | 2 | `member_flags` | `u16` | little | spec | `u16 0x00fc` |

Unstated regions:

- `49..100` (51 B): The compact-form tail is retained with the native record; no semantic field is assigned.

## `form_class_325_cage_table`

Spec §1.1.1 · layout: byte offsets · size: 1850 B

Offsets are relative to the class-325 Form primary indexed header. The 32-entry run starts at offset 41; each entry is 30 bytes. The class-325 frame length is 890 plus 30 times its cage count.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | indexed header |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | nine zero bytes at offset 11 |
| 20 | 1 | `list_marker` | `u8` | little | spec | `u8 1` at offset 20 · value `1` |
| 21 | 5 | `zero_run_5` | `bytes[5]` | little | spec | five zero bytes |
| 26 | 1 | `owner_marker` | `u8` | little | spec | a marked u64 result-record reference at offset 26 · value `1` |
| 27 | 8 | `owner_result_record_index` | `u64` | little | spec | a marked u64 result-record reference at offset 26 |
| 35 | 2 | `zero_run_2` | `bytes[2]` | little | spec | two zero bytes |
| 37 | 4 | `cage_count` | `u32` | little | spec | the u32 cage count at offset 37 · value `32` |
| 41 | 960 | `cage_entries` | `bytes[960]` | little | spec | The entries start at offset 41 and repeat every 30 bytes. |

Unstated regions:

- `1001..1850` (849 B): The fixed class-325 tail follows the 32-entry cage run.

## `form_class_325_cage_entry`

Spec §1.1.1 · layout: byte offsets · size: 30 B

Offsets are relative to one entry's base; the entry repeats every 30 bytes from class-325 offset 41.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `cage_object_marker` | `u8` | little | spec | a marked u64 class-289 cage-object reference · value `1` |
| 1 | 8 | `cage_object_record_index` | `u64` | little | spec | a marked u64 class-289 cage-object reference |
| 9 | 2 | `cage_object_zero` | `u16` | little | spec | `u16 0` · value `0` |
| 11 | 8 | `type_discriminator` | `u64` | little | spec | a u64 type discriminator |
| 19 | 1 | `companion_marker` | `u8` | little | spec | a marked u64 class-273 companion reference · value `1` |
| 20 | 8 | `companion_record_index` | `u64` | little | spec | a marked u64 class-273 companion reference |
| 28 | 2 | `companion_zero` | `u16` | little | spec | `u16 0` · value `0` |

## `form_serializer_frame_132`

Spec §1.1.1 · layout: byte offsets · size: 132 B

Offsets are relative to the serializer's primary indexed header. The LP-UTF16 entry-name span and the marked surface reference are variable-length fields within the fixed frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes after its indexed header |
| 21 | 4 | `entry_name_length` | `u32` | little | spec | LP-UTF16 blob-part entry name |

Unstated regions:

- `25..132` (107 B): The LP-UTF16 entry name, marked surface reference, and two-byte zero tail occupy this variable-length region; the complete serializer frame remains 132 bytes.

## `extrude_selection_member_fixed_frame`

Spec §3.1 · layout: byte offsets · size: 190 B

Offsets are relative to the member's indexed header. The UUID payloads occupy 36 UTF-16 code units each; the following indexed header is absent only at the stream-end boundary.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes |
| 21 | 8 | `local_identity` | `u64` | little | spec | a local persistent identity |
| 29 | 4 | `asset_uuid_length` | `u32` | little | spec | the LP-UTF16 asset UUID |
| 33 | 72 | `asset_uuid_utf16` | `bytes[72]` | little | spec | the LP-UTF16 asset UUID |
| 105 | 4 | `context_uuid_length` | `u32` | little | spec | an LP-UTF16 context UUID |
| 109 | 72 | `context_uuid_utf16` | `bytes[72]` | little | spec | an LP-UTF16 context UUID |
| 181 | 4 | `tail_kind` | `u32` | little | spec | `u32 2` |
| 185 | 1 | `tail_slot_marker` | `u8` | little | spec | an optional-slot marker |
| 186 | 4 | `tail_slot_value` | `u32` | little | spec | followed by u32 zero |

## `coil_compact_scope_discriminators`

Spec §3.1 · layout: byte offsets · size: 111 B

Offsets are relative to the primary indexed scope header. The ordered reference table and scope trailer follow this fixed discriminator block.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 20 | 4 | `operation` | `u32` | little | spec | Offset 20 value `1` creates a new body |
| 24 | 1 | `clockwise` | `u8` | little | spec | offset 24 is the clockwise Boolean |
| 26 | 4 | `structural_constant` | `u32` | little | spec | offset 26 is u32 `4` |
| 30 | 4 | `extent` | `u32` | little | spec | Offset 30 selects the driving dimensions |
| 92 | 4 | `section_placement` | `u32` | little | spec | Offset 92 selects the section position |
| 107 | 4 | `section_shape` | `u32` | little | spec | Offset 107 selects the section shape |

Unstated regions:

- `11..20` (9 B): The scope framing precedes the fixed Coil discriminators.
- `25..26` (1 B): The structural constant starts at offset 26.
- `34..92` (58 B): The scope reference table and parameter-specific lanes follow the extent discriminator.
- `96..107` (11 B): The fixed discriminator block retains the section-shape lane at offset 107.

## `coil_long_scope_fixed_prologue`

Spec §3.1 · layout: byte offsets · size: 52 B

Offsets are relative to the primary indexed scope header. The two marked references repeat ordered-reference ordinals four and eight; their target records are dynamic indexed records.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte indexed header |
| 11 | 11 | `zero_run_11` | `bytes[11]` | little | spec | All forms store eleven zero bytes at offsets 11 through 21 |
| 22 | 4 | `operation` | `u32` | little | spec | a u32 at offset 22 |
| 26 | 4 | `structural_constant` | `u32` | little | spec | u32 `1` at offset 26 |
| 30 | 11 | `fifth_reference` | `bytes[11]` | little | spec | Marked references at offsets 30 and 41 repeat the fifth and ninth ordered scope references. |
| 41 | 11 | `ninth_reference` | `bytes[11]` | little | spec | Marked references at offsets 30 and 41 repeat the fifth and ninth ordered scope references. |

## `coil_long_scope_matrix`

Spec §3.1 · layout: byte offsets · size: 128 B

The block begins at primary indexed scope offset 77. Its final row is `(0, 0, 0, 1)`; the 572-byte form carries Boolean operations and the 578-byte form carries new body.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 128 | `matrix` | `f64[16]` | little | spec | stores a finite 16-value f64 matrix at offset 77 |

## `coil_compact_persistent_selection_prefix`

Spec §3.1 · layout: byte offsets · size: 40 B

Offsets are relative to the first placement selection header. The asset and context UUID payloads follow the fixed UTF-16 length fields and therefore have variable length.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 21 | 1 | `nested_selection_marker` | `u8` | little | spec | u8 `1` at offset 21 |
| 22 | 4 | `nested_record_index` | `u32` | little | spec | the nested record index at offset 22 |
| 32 | 4 | `asset_presence` | `u32` | little | spec | u32 `1` at offset 32 |
| 36 | 4 | `asset_uuid_length` | `u32` | little | spec | the asset UUID's UTF-16 code-unit count at offset 36 |

Unstated regions:

- `0..11` (11 B): The indexed selection header occupies the first 11 bytes.
- `11..21` (10 B): The persistent prefix stores zero bytes at offsets 11 through 20.
- `26..32` (6 B): The persistent prefix stores zero bytes at offsets 26 through 31.

## `work_point_sketch_point_identity`

Spec §3.1 · layout: byte offsets · size: 41 B

Offsets are relative to the identity record's indexed header. The following indexed record begins at offset 41.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 1 | `presence` | `u8` | little | spec | offset 20 is the presence byte `01` |
| 25 | 4 | `sketch_record_index` | `u32` | little | spec | offsets 25 through 28 are the owning Sketch entity record index |
| 33 | 4 | `point_persistent_id` | `u32` | little | spec | offsets 33 through 36 are the sketch-point persistent id |

Unstated regions:

- `0..11` (11 B): The indexed identity header occupies the first 11 bytes.
- `11..20` (9 B): The direct sketch-point identity stores zero bytes at offsets 11 through 19.
- `21..25` (4 B): The first marked identity slot is zero in this selection form.
- `29..33` (4 B): The second marked identity slot is zero in this selection form.
- `37..41` (4 B): The identity record stores zero bytes at offsets 37 through 40.

## `class_338_sketch_curve_identity`

Spec §3.1 · layout: byte offsets · size: 49 B

Offsets are relative to the class-`361` identity record's indexed header. The following indexed record begins at offset 49.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 9 | `zero_prefix` | `bytes[9]` | little | spec | offsets `11..19` are zero |
| 20 | 1 | `presence` | `u8` | little | spec | offset `20` is presence byte `1` |
| 33 | 4 | `owner_record_index` | `u32` | little | spec | offset `33` is a u32 Sketch entity record index |
| 37 | 4 | `owner_high_zero` | `u32` | little | spec | offset `37` is a zero u32 |
| 41 | 4 | `curve_persistent_id` | `u32` | little | spec | offset `41` is a u32 curve persistent identity |
| 45 | 4 | `curve_high_zero` | `u32` | little | spec | offset `45` is a zero u32 |

Unstated regions:

- `0..11` (11 B): The indexed identity header occupies the first 11 bytes.
- `21..33` (12 B): The first three u32 identity lanes are zero in this selection form.

## `coil_modern_selection_prefix`

Spec §3.1 · layout: byte offsets · size: 41 B

Offsets are relative to the class-286 first placement selection header. The asset and context UUID payloads follow the fixed UTF-16 length fields and therefore have variable length.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 22 | 1 | `nested_selection_marker` | `u8` | little | spec | offset 22 is u8 `1` |
| 23 | 4 | `nested_record_index` | `u32` | little | spec | offset 23 is the nested record index |
| 33 | 4 | `asset_presence` | `u32` | little | spec | offset 33 is u32 `1` |
| 37 | 4 | `asset_uuid_length` | `u32` | little | spec | offset 37 is the asset UUID's UTF-16 code-unit count |

Unstated regions:

- `0..11` (11 B): The indexed selection header occupies the first 11 bytes.
- `11..22` (11 B): The modern selection prefix stores zero bytes at offsets 11 through 21.
- `27..33` (6 B): The modern selection prefix stores zero bytes at offsets 27 through 32.

## `coil_compact_face_selection_prefix`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the first placement selection header. The asset and context UUID payloads follow the fixed UTF-16 length fields and therefore have variable length.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 23 | 1 | `nested_selection_marker` | `u8` | little | spec | u8 `1` at offset 23 |
| 24 | 4 | `nested_record_index` | `u32` | little | spec | the nested record index at offset 24 |
| 34 | 1 | `asset_presence` | `u8` | little | spec | u8 `1` at offset 34 |
| 38 | 4 | `asset_uuid_length` | `u32` | little | spec | the asset UUID's UTF-16 code-unit count at offset 38 |

Unstated regions:

- `0..11` (11 B): The indexed selection header occupies the first 11 bytes.
- `11..23` (12 B): Its prefix stores zero bytes at offsets 11 through 22.
- `28..34` (6 B): Its prefix stores zero bytes at offsets 28 through 33.
- `35..38` (3 B): Its prefix stores zero bytes at offsets 35 through 37.

## `coil_legacy_placement_identity_frame`

Spec §3.1 · layout: byte offsets · size: 186 B

Offsets are relative to the class-395 second placement carrier's primary indexed header. The carrier is the identity form of the legacy eight-reference Coil scope.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 48 | 1 | `leading_reference_marker` | `u8` | little | spec | offset 48 is a marked reference with index zero |
| 49 | 4 | `leading_reference_index` | `u32` | little | spec | offset 48 is a marked reference with index zero |
| 76 | 4 | `prologue_value` | `u32` | little | spec | offset 76 is u32 `2` |
| 84 | 4 | `prologue_flag` | `u32` | little | spec | offset 84 is u32 `1` |
| 88 | 1 | `selection_reference_marker` | `u8` | little | spec | A marked reference at offset 88 names the first placement reference |
| 89 | 4 | `selection_record_index` | `u32` | little | spec | A marked reference at offset 88 names the first placement reference |
| 101 | 4 | `selection_flag` | `u32` | little | spec | offset 101 is u32 `1` |
| 105 | 1 | `auxiliary_reference_marker` | `u8` | little | spec | A second marked reference at offset 105 is nonzero |
| 106 | 4 | `auxiliary_record_index` | `u32` | little | spec | A second marked reference at offset 105 is nonzero |
| 120 | 4 | `tail_value` | `u32` | little | spec | offset 120 is u32 `4` |
| 134 | 4 | `intermediate_selector` | `u32` | little | spec | offset 134 is u32 `109` |
| 138 | 8 | `carrier_scalar` | `f64` | little | spec | offset 138 is a positive finite f64 |
| 146 | 4 | `tail_selector` | `u32` | little | spec | offset 146 is u32 `109` |
| 150 | 1 | `successor_reference_marker` | `u8` | little | spec | Marked references at offsets 150 and 163 name the transform record plus two and plus one |
| 151 | 4 | `successor_record_index` | `u32` | little | spec | Marked references at offsets 150 and 163 name the transform record plus two and plus one |
| 163 | 1 | `predecessor_reference_marker` | `u8` | little | spec | Marked references at offsets 150 and 163 name the transform record plus two and plus one |
| 164 | 4 | `predecessor_record_index` | `u32` | little | spec | Marked references at offsets 150 and 163 name the transform record plus two and plus one |
| 175 | 1 | `owner_reference_marker` | `u8` | little | spec | the marked reference at offset 175 names the owning Coil scope |
| 176 | 4 | `owner_scope_record_index` | `u32` | little | spec | the marked reference at offset 175 names the owning Coil scope |

Unstated regions:

- `0..48` (48 B): The indexed carrier header and fixed prologue precede the first marked reference.
- `53..59` (6 B): The first marked reference has six zero trailing bytes.
- `59..76` (17 B): The legacy carrier stores zero bytes at offsets 59 through 75.
- `80..84` (4 B): The carrier stores zero bytes at offsets 80 through 83.
- `93..99` (6 B): The selection reference has six zero trailing bytes.
- `99..101` (2 B): The carrier stores zero bytes at offsets 99 and 100.
- `110..116` (6 B): The auxiliary reference has six zero trailing bytes.
- `116..120` (4 B): The carrier stores zero bytes at offsets 116 through 119.
- `124..134` (10 B): The carrier stores zero bytes at offsets 124 through 133.
- `155..161` (6 B): The successor reference has six zero trailing bytes.
- `161..163` (2 B): The carrier stores zero bytes at offsets 161 and 162.
- `168..174` (6 B): The predecessor reference has six zero trailing bytes.
- `174..175` (1 B): The carrier stores zero at offset 174.
- `180..186` (6 B): The owner reference has six zero trailing bytes.

## `coil_compact_placement_identity_frame`

Spec §3.1 · layout: byte offsets · size: 213 B

Offsets are relative to the second ordered placement carrier's indexed header. The identity form omits the matrix block.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 55 | 1 | `placement_marker` | `u8` | little | spec | Its marker at offset 55 is u8 `1` |
| 56 | 9 | `identity_zero_run` | `bytes[9]` | little | spec | stores zero bytes at offsets 56 through 64 |
| 65 | 1 | `identity_marker` | `u8` | little | spec | stores u8 `1` at offset 65 |

Unstated regions:

- `0..55` (55 B): The fixed placement envelope precedes the identity marker block.
- `66..213` (147 B): The identity form omits the explicit matrix block and retains the remaining carrier bytes natively.

## `coil_modern_placement_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 315 B

Offsets are relative to the class-450 second placement carrier's primary indexed header. The matrix is row-major and its translation values are in centimetres.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 50 | 128 | `matrix` | `f64[16]` | little | spec | sixteen row-major f64 matrix values begin at offset 50 |
| 204 | 4 | `constant_512` | `u32` | little | spec | offset 204 is u32 `512` |
| 212 | 4 | `constant_256` | `u32` | little | spec | offset 212 is u32 `256` |
| 217 | 11 | `selection_reference` | `bytes[11]` | little | spec | A marked reference at offset 217 names the first placement reference |
| 230 | 4 | `selection_flag` | `u32` | little | spec | offset 230 is u32 `1` |
| 234 | 11 | `auxiliary_reference` | `bytes[11]` | little | spec | a marked reference at offset 234 names the transform record plus 25 |
| 248 | 8 | `constant_1024` | `u64` | little | spec | offset 248 is u64 `1024` |
| 256 | 8 | `identity_lane_prefix` | `u64` | little | spec | offset 256 is u64 `0x7000000000000000` |
| 268 | 8 | `identity_lane` | `u64` | little | spec | offset 268 is a nonzero u64 whose most-significant byte is `0x70` |
| 279 | 11 | `successor_reference` | `bytes[11]` | little | spec | Marked references at offsets 279, 292, and 304 name |
| 292 | 11 | `predecessor_reference` | `bytes[11]` | little | spec | the transform record plus two, the transform record plus one |
| 304 | 11 | `owner_reference` | `bytes[11]` | little | spec | and the owning Coil scope |

Unstated regions:

- `0..11` (11 B): The indexed carrier header occupies the first eleven bytes.
- `11..50` (39 B): Bytes 11 through 49 are zero.
- `178..204` (26 B): Bytes 178 through 203 are zero.
- `208..212` (4 B): Bytes 208 through 211 are zero.
- `216..217` (1 B): One zero byte precedes the first marked reference.
- `228..230` (2 B): Bytes 228 and 229 are zero.
- `245..248` (3 B): Three zero bytes precede the u64 constant.
- `264..268` (4 B): Bytes 264 through 267 are zero.
- `276..279` (3 B): Bytes 276 through 278 are zero.
- `290..292` (2 B): Bytes 290 through 291 are zero.
- `303..304` (1 B): Byte 303 is zero.

## `coil_compact_placement_owner_identity_frame`

Spec §3.1 · layout: byte offsets · size: 233 B

Offsets are relative to the second ordered placement carrier's indexed header. The owner reference closes the carrier to its containing Coil scope.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 222 | 1 | `owner_reference_marker` | `u8` | little | spec | offset 222 is a marked reference |
| 223 | 4 | `owner_scope_record_index` | `u32` | little | spec | the u32 at offset 223 equals the owning Coil scope record index |
| 227 | 6 | `owner_reference_tail` | `bytes[6]` | little | spec | bytes 227 through 232 are zero |

Unstated regions:

- `0..213` (213 B): The fixed identity carrier precedes the owner-reference extension.
- `213..222` (9 B): Its bytes 213 through 221 are zero.

## `work_plane_legacy_class_290_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 325 B

Offsets are relative to the class-290 primary indexed placement header paired with class 262. The four-byte marker distinguishes this prefix from the zero-prefix 325-byte placement forms.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 45 | 4 | `prefix_marker` | `bytes[4]` | little | spec | bytes `0x01, 0x01, 0x00, 0x00` at offsets 45 through 48 |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | a row-major 4×4 f64 local-to-model matrix at offset 49 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..45` (34 B): The fixed class-290 placement prefix is zero.
- `177..325` (148 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_legacy_325_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 325 B

Offsets are relative to any of the class-308, class-320, class-364, class-380, or class-431 primary indexed placement headers. Their paired classes are 257, 258, 263, 262, and 257 respectively.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | The class-308/257, class-320/258, class-364/263, class-380/262, and class-431/257 placement frames use a second 325-byte layout |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..49` (38 B): The fixed placement prefix is zero.
- `177..325` (148 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_legacy_class_256_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 325 B

Offsets are relative to the class-256 primary indexed placement header paired with class 262. The two-byte lane before the final zero pair is opaque.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 45 | 2 | `opaque_u16` | `u16` | little | spec | bytes 45 through 46 are an opaque little-endian u16 |
| 47 | 2 | `zero_pair` | `bytes[2]` | little | spec | bytes 47 through 48 are zero |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | the class-256/262 placement frame stores its row-major 4×4 f64 local-to-model matrix at offset 49 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..45` (34 B): Bytes 11 through 44 are zero.
- `177..325` (148 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_legacy_class_337_325_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 325 B

Offsets are relative to the class-337 primary indexed placement header paired with class 266. The two-byte lane before the final zero pair is opaque.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 45 | 2 | `opaque_u16` | `u16` | little | spec | bytes 45 through 46 are an opaque little-endian u16 |
| 47 | 2 | `zero_pair` | `bytes[2]` | little | spec | bytes 47 through 48 are zero |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | The class-337/266 WorkPlane placement frame is 325 bytes and uses the same offsets |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..45` (34 B): Bytes 11 through 44 are zero.
- `177..325` (148 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_legacy_321_opaque_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 321 B

Offsets are relative to the class-341 primary indexed placement header paired with class 261, or the class-346 primary indexed placement header paired with class 262. The two-byte lane before the final zero pair is opaque.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 45 | 2 | `opaque_u16` | `u16` | little | spec | bytes 45 through 46 are an opaque little-endian u16 |
| 47 | 2 | `zero_pair` | `bytes[2]` | little | spec | bytes 47 through 48 are zero |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | The class-341/261 and class-346/262 placement frames use a 321-byte form |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..45` (34 B): Bytes 11 through 44 are zero.
- `177..321` (144 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_legacy_337_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 337 B

Offsets are relative to either the class-350 or class-409 primary indexed placement header paired with class 258. The placement carriers share a 39-byte zero prefix and matrix offset.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 50 | 128 | `matrix` | `f64[16]` | little | spec | The 337-byte class-350/258 and class-409/258 placement frames use a third 337-byte layout |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..50` (39 B): The fixed placement prefix is zero.
- `178..337` (159 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_legacy_class_400_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 345 B

Offsets are relative to the first ordered placement member's class-400 indexed header. The class-400 tail retains the construction references after the solved matrix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | the placement frame stores 38 zero bytes after its indexed header and the row-major 4×4 f64 local-to-model matrix at offset 49 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..49` (38 B): The class-400 placement prefix is zero.
- `177..345` (168 B): The class-400 construction-reference tail is retained as a named opaque carrier.

## `work_axis_direct_carrier_class_297`

Spec §3.1 · layout: byte offsets · size: 215 B

Offsets are relative to the class-297 primary indexed axis carrier paired with class 262.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 21 | 4 | `value_count` | `u32` | little | spec | u32 value count 8 at offset 21 |
| 25 | 64 | `axis_values` | `f64[8]` | little | spec | eight f64 values at offset 25 |
| 89 | 4 | `reference_count` | `u32` | little | spec | u32 reference count 6 at offset 89 |
| 93 | 4 | `reference_preamble` | `u32` | little | spec | u32 value 1 at offset 93 |

Unstated regions:

- `0..11` (11 B): The indexed carrier header occupies the first eleven bytes.
- `11..21` (10 B): Bytes 11 through 20 are zero.
- `97..215` (118 B): The generation-specific construction/reference tail ends at the paired header.

## `work_axis_direct_carrier_class_335`

Spec §3.1 · layout: byte offsets · size: 195 B

Offsets are relative to the class-335 primary indexed axis carrier paired with class 258.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 21 | 4 | `value_count` | `u32` | little | spec | u32 value count 8 at offset 21 |
| 25 | 64 | `axis_values` | `f64[8]` | little | spec | eight f64 values at offset 25 |
| 89 | 4 | `reference_count` | `u32` | little | spec | u32 reference count 6 at offset 89 |
| 93 | 4 | `reference_preamble` | `u32` | little | spec | u32 value 1 at offset 93 |

Unstated regions:

- `0..11` (11 B): The indexed carrier header occupies the first eleven bytes.
- `11..21` (10 B): Bytes 11 through 20 are zero.
- `97..195` (98 B): The generation-specific construction/reference tail ends at the paired header.

## `coil_compact_placement_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 341 B

Offsets are relative to the second ordered placement carrier's indexed header. The matrix is row-major and its translation values are in centimetres.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 55 | 1 | `placement_marker` | `u8` | little | spec | Its marker at offset 55 is u8 `1` |
| 56 | 9 | `explicit_zero_run` | `bytes[9]` | little | spec | stores zero bytes at offsets 56 through 64 |
| 65 | 1 | `explicit_form_marker` | `u8` | little | spec | stores u8 `0` at offset 65 |
| 66 | 128 | `matrix` | `f64[16]` | little | spec | stores 16 row-major f64 values at offset 66 |

Unstated regions:

- `0..55` (55 B): The fixed placement envelope precedes the explicit marker block.
- `194..341` (147 B): The carrier tail is not assigned a semantic field.

## `class_403_revolve_scope_frame`

Spec §3.1 · layout: byte offsets · size: 387 B

Offsets are relative to the primary indexed header. The frame has eight ordered references; its fixed operation prefix and angle-owner reference are typed, and the remaining marked references retain their source envelope.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 21 | 4 | `operation` | `u32` | little | spec | Offset 21 stores the result operation |
| 25 | 4 | `extent_kind` | `u32` | little | spec | offset 25 stores extent kind `2` |
| 29 | 1 | `direction_kind` | `u8` | little | spec | offset 29 stores direction kind `0` |
| 30 | 1 | `envelope_marker` | `u8` | little | spec | offset 30 stores marker `1` |
| 34 | 1 | `angle_reference_marker` | `u8` | little | spec | A marked angle-owner reference starts at offset 34 |
| 35 | 4 | `angle_record_index` | `u32` | little | spec | A marked angle-owner reference starts at offset 34 |
| 107 | 4 | `guid_code_unit_count` | `u32` | little | spec | A 36-code-unit null GUID starts at offset 107 |
| 111 | 72 | `guid_utf16` | `bytes[72]` | little | spec | A 36-code-unit null GUID starts at offset 107 |
| 186 | 4 | `reference_count` | `u32` | little | spec | the ordered-reference count is `8` at offset 186 |
| 278 | 4 | `history_state_id` | `u32` | little | spec | the history-state field is at offset 278 |
| 282 | 4 | `kind_code_unit_count` | `u32` | little | spec | the LP-UTF16 `Revolve` kind has seven code units at offset 282 |
| 286 | 14 | `kind_utf16` | `bytes[14]` | little | spec | the LP-UTF16 `Revolve` kind has seven code units at offset 282 |
| 300 | 4 | `feature_ordinal` | `u32` | little | spec | the feature ordinal is at offset 300 |
| 341 | 4 | `previous_history_state_id` | `u32` | little | spec | The previous history-state field is at offset 341 |

Unstated regions:

- `0..21` (21 B): The indexed header and the class-403/258 prefix before the operation are outside this field run.
- `31..34` (3 B): Three zero bytes separate the fixed prefix from the angle-owner reference.
- `39..107` (68 B): The auxiliary marked references and their separators occupy offsets 39 through 106.
- `183..186` (3 B): Three zero bytes precede the ordered-reference count.
- `190..278` (88 B): The eight eleven-byte ordered references occupy offsets 190 through 277.
- `304..341` (37 B): The fixed history-state tail occupies offsets 304 through 340.
- `345..387` (42 B): The remaining scope tail precedes the paired header at offset 387.

## `marker_one_revolve_prologue`

Spec §3.1 · layout: byte offsets · size: 38 B

Offsets are relative to the Revolve primary indexed header. Every marker-one class pair uses the same fixed prologue.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 1 | `marker` | `u8` | little | spec | Offset 20 is marker `1` |
| 21 | 4 | `zero_value` | `u32` | little | spec | offset 21 is u32 zero |
| 25 | 4 | `operation` | `u32` | little | spec | offset 25 stores the result operation |
| 29 | 4 | `extent_kind` | `u32` | little | spec | offset 29 stores extent kind `2` |
| 33 | 1 | `direction_kind` | `u8` | little | spec | offset 33 stores direction kind `0` |
| 34 | 4 | `structural_constant` | `u32` | little | spec | offset 34 stores u32 one |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding marker-one Revolve envelope are outside this field run.

## `current_extrude_operation_fields`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the result-operation u32 of a current reference-aware Extrude scope. The seven variable-width nullable reference slots follow at offset 42.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `operation` | `u32` | little | spec | stores its result-operation u32 |
| 4 | 4 | `direction` | `u32` | little | spec | The two immediately following u32 values are the travel direction and the face-extend option |
| 8 | 4 | `face_extend` | `u32` | little | spec | The two immediately following u32 values are the travel direction and the face-extend option |
| 12 | 1 | `direction_reversed` | `u8` | little | spec | Offset `operation + 12` is the direction-reversal Boolean |
| 13 | 1 | `geometry_kind` | `u8` | little | spec | offset `operation + 13` is the geometry-kind Boolean |
| 14 | 1 | `start_support` | `u8` | little | spec | offset `operation + 14` is the start-support byte |
| 15 | 3 | `zero_run_3` | `bytes[3]` | little | spec | stores three zero bytes at `operation + 15` through `operation + 17` |
| 18 | 24 | `profile_normal` | `f64[3]` | little | spec | Its profile normal is three contiguous unit-length f64 values at `operation + 18` |

## `current_extrude_non_target_extent_pair`

Spec §3.1 · layout: byte offsets · size: 17 B

Offsets are relative to the first-side extent u32. This frame applies when the first-side extent is not the to-entity value 2.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `first_side_extent` | `u32` | little | spec | A first-side value other than `2` |
| 4 | 9 | `zero_run_9` | `bytes[9]` | little | spec | is followed by nine zero bytes |
| 13 | 4 | `second_side_extent` | `u32` | little | spec | the second-side value at first-side offset `+13` |

## `current_extrude_shape_target_extent_prefix`

Spec §3.1 · layout: byte offsets · size: 9 B

Offsets are relative to the repeated target-group ordinal. The target payload follows the first-side extent; the second-side extent is four bytes before the scope reference-count field.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `target_scope_reference_ordinal` | `u32` | little | spec | A u32 zero-based scope-reference ordinal immediately after the seventh slot |
| 4 | 1 | `zero_separator` | `u8` | little | spec | one zero byte follows the ordinal |
| 5 | 4 | `first_side_extent` | `u32` | little | spec | the first-side value `2` follows that byte |

## `early_distance_extrude_absent_prefix`

Spec §3.1 · layout: byte offsets · size: 34 B

Offsets are relative to the early distance-only Extrude primary indexed header. The scope reference-count field is at offset 208.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 1 | `absent_prefix` | `u8` | little | spec | An absent field is one zero byte |
| 21 | 4 | `operation` | `u32` | little | spec | places the operation u32 at offset 21 |
| 25 | 4 | `extent_kind` | `u32` | little | spec | The extent-kind u32 value `2` follows the operation |
| 29 | 1 | `direction_reversed` | `u8` | little | spec | The direction-reversal Boolean follows the extent kind |
| 30 | 4 | `geometry_kind` | `u32` | little | spec | A u32 geometry-kind discriminator follows the Boolean |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding scope envelope are outside the fixed prologue fields.

## `early_distance_extrude_present_prefix`

Spec §3.1 · layout: byte offsets · size: 38 B

Offsets are relative to the early distance-only Extrude primary indexed header. The scope reference-count field is at offset 212.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 1 | `present_prefix_marker` | `u8` | little | spec | A present field is byte `01` |
| 21 | 4 | `prefix_value` | `u32` | little | spec | and u32 zero |
| 25 | 4 | `operation` | `u32` | little | spec | places the operation at offset 25 |
| 29 | 4 | `extent_kind` | `u32` | little | spec | The extent-kind u32 value `2` follows the operation |
| 33 | 1 | `direction_reversed` | `u8` | little | spec | The direction-reversal Boolean follows the extent kind |
| 34 | 4 | `geometry_kind` | `u32` | little | spec | A u32 geometry-kind discriminator follows the Boolean |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding scope envelope are outside the fixed prologue fields.

## `shifted_extrude_prologue`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the shifted Extrude primary indexed header. The operation fields end at the start-support byte; extent lanes and the ordered reference table follow in the enclosing scope.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | stores u32 `1` at primary-header offset 20 |
| 24 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes at offsets 24 through 26 |
| 27 | 4 | `operation` | `u32` | little | spec | stores the result-operation u32 at offset 27 |
| 31 | 4 | `direction` | `u32` | little | spec | the travel direction at offset 31 |
| 35 | 4 | `face_extend` | `u32` | little | spec | the face-extend option at offset 35 |
| 39 | 1 | `direction_reversed` | `u8` | little | spec | the direction-reversal Boolean at offset 39 |
| 40 | 1 | `geometry_kind` | `u8` | little | spec | the geometry-kind Boolean at offset 40 |
| 41 | 1 | `start_support` | `u8` | little | spec | the start-support byte at offset 41 |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding shifted-operation envelope are outside this field run.

## `legacy_class_397_symmetric_extrude_frame`

Spec §3.1 · layout: byte offsets · size: 473 B

Offsets are relative to the class-397 primary indexed header. The paired class-262 header begins at offset 473. The class-local parameter/reference envelope is retained between the extent pair and the GUID; the fixed extent and reference-count fields are the admission fields.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | stores u32 `1` at primary-header offset 20 · value `1` |
| 27 | 4 | `operation` | `u32` | little | spec | stores the result-operation u32 at offset 27 |
| 31 | 4 | `direction` | `u32` | little | spec | travel direction `3` (symmetric) · value `3` |
| 35 | 4 | `face_extend` | `u32` | little | spec | face-extend value `2` · value `2` |
| 39 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal |
| 40 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind |
| 41 | 1 | `start_support` | `u8` | little | spec | start support |
| 45 | 24 | `profile_normal` | `f64[3]` | little | spec | finite unit profile normal |
| 69 | 57 | `reference_slots` | `bytes[57]` | little | spec | seven nullable slots |
| 126 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent `1` (distance) · value `1` |
| 139 | 4 | `second_side_extent` | `u32` | little | spec | second-side extent `1` (distance) · value `1` |
| 203 | 76 | `guid` | `bytes[76]` | little | spec | LP-UTF16 GUID: 36 code units |
| 282 | 4 | `reference_count` | `u32` | little | spec | ordered reference count `8` · value `8` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding shifted-operation envelope are outside this fixed frame prefix.
- `24..27` (3 B): Three zero bytes separate the prefix constant from the result operation.
- `42..45` (3 B): Three zero bytes precede the profile normal.
- `130..139` (9 B): Nine zero bytes separate the two extent values.
- `143..203` (60 B): The class-local parameter/reference envelope precedes the GUID.
- `279..282` (3 B): Three zero bytes separate the GUID from the ordered reference count.
- `286..473` (187 B): The ordered reference entries and common scope tail precede the paired indexed header at offset 473.

## `shifted_reference_aware_extrude_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 296 B

Offsets are relative to the primary indexed header. The fixed prefix ends at the u32 reference count; the ordered reference table has 13 entries for the 538-byte class pairs 357/258, 275/262, 361/262, 349/266, and 397/262, and 11 entries for class pair 323/263, and the scope tail follows it. The final zero high byte of the 36-code-unit GUID is shared with the second-side extent lane.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | stores u32 `1` · value `1` |
| 24 | 3 | `zero_run_3` | `bytes[3]` | little | spec | `+24..+26` |
| 27 | 4 | `operation` | `u32` | little | spec | result operation: `1 = join` |
| 31 | 4 | `direction` | `u32` | little | spec | travel direction `2` · value `2` |
| 35 | 4 | `face_extend` | `u32` | little | spec | face-extend option `1` · value `1` |
| 39 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal |
| 40 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind |
| 41 | 1 | `start_support` | `u8` | little | spec | start support |
| 42 | 3 | `zero_run_3_after_start` | `bytes[3]` | little | spec | `+42..+44` |
| 45 | 24 | `profile_normal` | `f64[3]` | little | spec | finite unit profile normal |
| 69 | 47 | `reference_slots` | `bytes[47]` | little | spec | three absent and four present nullable slots |
| 116 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent `2` · value `2` |
| 120 | 11 | `first_side_owner_reference` | `bytes[11]` | little | spec | `Side1Offset` owner reference |
| 131 | 4 | `first_side_padding` | `bytes[4]` | little | spec | `+131..+134` |
| 135 | 4 | `first_side_discriminant` | `u32` | little | spec | `+135` · value `1` |
| 139 | 4 | `first_side_payload` | `u32` | little | spec | `+139` · value `2` |
| 143 | 1 | `first_side_separator` | `u8` | little | spec | `+143` · value `0` |
| 144 | 11 | `second_side_offset_reference` | `bytes[11]` | little | spec | `Side2Offset` owner reference |
| 155 | 4 | `second_side_offset_padding` | `bytes[4]` | little | spec | `+155..+158` |
| 159 | 11 | `second_side_taper_reference` | `bytes[11]` | little | spec | `Side2TaperAngle` owner reference |
| 170 | 5 | `second_side_taper_padding` | `bytes[5]` | little | spec | `+170..+174` |
| 175 | 4 | `profile_group_count` | `u32` | little | spec | `+175` · value `1` |
| 179 | 11 | `profile_group_reference` | `bytes[11]` | little | spec | profile construction-group reference |
| 190 | 8 | `profile_group_padding` | `bytes[8]` | little | spec | zero for the 538-byte class pairs |
| 198 | 4 | `body_group_count` | `u32` | little | spec | body construction-group count `1` for the 538-byte class pairs · value `1` |
| 202 | 11 | `body_group_reference` | `bytes[11]` | little | spec | body construction-group reference for the 538-byte class pairs |
| 213 | 75 | `body_group_guid_prefix` | `bytes[75]` | little | spec | UTF-16 code-unit count `36` |
| 288 | 4 | `second_side_extent` | `u32` | little | spec | second-side extent `0` · value `0` |
| 292 | 4 | `reference_count` | `u32` | little | spec | `+292` |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding scope envelope are outside this fixed prefix.

## `shifted_reference_aware_extrude_class_323_tail`

Spec §3.1 · layout: byte offsets · size: 288 B

Offsets are relative to the primary indexed header. The class-specific tail moves the trailing-reference count and marked reference to +190 and +194, leaves zero padding at +205..+212, and keeps the 36-code-unit GUID prefix at +213.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 175 | 4 | `profile_group_count` | `u32` | little | spec | `+175` |
| 179 | 11 | `profile_group_reference` | `bytes[11]` | little | spec | profile construction-group reference |
| 190 | 4 | `trailing_reference_count` | `u32` | little | spec | trailing-reference count `+190` · value `1` |
| 194 | 11 | `trailing_reference` | `bytes[11]` | little | spec | unlisted trailing reference `+194` |
| 205 | 8 | `trailing_reference_padding` | `bytes[8]` | little | spec | zero padding `+205..+212` |
| 213 | 75 | `guid_prefix` | `bytes[75]` | little | spec | GUID prefix remains at `+213` |

Unstated regions:

- `0..175` (175 B): The indexed header and the shared class-specific prefix precede this tail.

## `shifted_reference_aware_extrude_class_323_symmetric_prefix`

Spec §3.1 · layout: byte offsets · size: 276 B

Offsets are relative to the primary indexed header. This class-specific prefix stores both symmetric-through-all extent discriminators before the grouped-reference tail and ends at the u32 reference count.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 116 | 4 | `first_side_extent` | `u32` | little | spec | symmetric-through-all first-side extent `4` · value `4` |
| 120 | 9 | `first_side_padding` | `bytes[9]` | little | spec | zero bytes at `+120..+128` |
| 129 | 4 | `second_side_extent` | `u32` | little | spec | symmetric-through-all second-side extent `4` · value `4` |
| 133 | 6 | `second_side_padding` | `bytes[6]` | little | spec | zero bytes at `+133..+138` |
| 139 | 11 | `symmetric_extent_reference` | `bytes[11]` | little | spec | marked symmetric-extent reference |
| 150 | 5 | `symmetric_extent_padding` | `bytes[5]` | little | spec | zero bytes at `+150..+154` |
| 155 | 4 | `profile_group_count` | `u32` | little | spec | profile construction-group count `1` · value `1` |
| 159 | 11 | `profile_group_reference` | `bytes[11]` | little | spec | profile construction-group reference |
| 170 | 8 | `profile_group_padding` | `bytes[8]` | little | spec | zero bytes at `+170..+177` |
| 178 | 4 | `trailing_reference_count` | `u32` | little | spec | trailing-reference count `1` · value `1` |
| 182 | 11 | `trailing_reference` | `bytes[11]` | little | spec | marked trailing reference |
| 193 | 76 | `guid_prefix` | `bytes[76]` | little | spec | UTF-16 code-unit count `36` |
| 269 | 3 | `reference_count_padding` | `bytes[3]` | little | spec | zero bytes at `+269..+271` |
| 272 | 4 | `reference_count` | `u32` | little | spec | ordered reference count `10` · value `10` |

Unstated regions:

- `0..116` (116 B): The indexed header and shared shifted reference-aware prologue precede the symmetric extent lane.

## `compact_shifted_extrude_prologue`

Spec §3.1 · layout: byte offsets · size: 41 B

Offsets are relative to the compact legacy Extrude primary indexed header. The operation fields end at the start-support byte; the one-sided extent lane and ordered reference table follow in the enclosing scope.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | The compact legacy Extrude prologue stores u32 `1` at primary-header offset 20 |
| 24 | 2 | `zero_run_2` | `bytes[2]` | little | spec | zero bytes at offsets 24 through 25 |
| 26 | 4 | `operation` | `u32` | little | spec | stores the result-operation u32 at offset 26 |
| 30 | 4 | `direction` | `u32` | little | spec | the travel direction at offset 30 |
| 34 | 4 | `face_extend` | `u32` | little | spec | the face-extend option at offset 34 |
| 38 | 1 | `direction_reversed` | `u8` | little | spec | the direction-reversal Boolean at offset 38 |
| 39 | 1 | `geometry_kind` | `u8` | little | spec | the geometry-kind Boolean at offset 39 |
| 40 | 1 | `start_support` | `u8` | little | spec | the start-support byte at offset 40 |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding compact-operation envelope are outside this field run.

## `compact_shifted_extrude_extent_and_table_prefix`

Spec §3.1 · layout: byte offsets · size: 255 B

Offsets are relative to the compact legacy Extrude primary indexed header. The one-sided distance and symmetric-distance forms use the two extent values shown here; the ordered reference table begins at the final field.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 105 | 4 | `first_side_extent` | `u32` | little | spec | the first-side extent is at offset 105 |
| 109 | 4 | `second_side_extent` | `u32` | little | spec | the second-side extent is at offset 109 |
| 251 | 4 | `reference_count` | `u32` | little | spec | the scope reference-count field is at offset 251 |

Unstated regions:

- `0..105` (105 B): The compact prologue and its intervening reference envelope precede the selected extent lane.
- `113..251` (138 B): The remaining compact extent envelope precedes the ordered reference-count field.

## `compact_shifted_extrude_mixed_extent_and_table_prefix`

Spec §3.1 · layout: byte offsets · size: 285 B

Offsets are relative to the compact legacy Extrude primary indexed header. The mixed two-sided form uses the two extent values shown here; the ordered reference table begins at the final field.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 124 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent discriminator `1` |
| 128 | 4 | `second_side_extent` | `u32` | little | spec | second-side extent discriminator `2` |
| 281 | 4 | `reference_count` | `u32` | little | spec | Its scope reference-count field is at offset 281 |

Unstated regions:

- `0..124` (124 B): The compact prologue and its intervening reference envelope precede the mixed extent lane.
- `132..281` (149 B): The mixed side-reference envelope precedes the ordered reference-count field.

## `marked_shifted_extrude_prologue`

Spec §3.1 · layout: byte offsets · size: 43 B

Offsets are relative to the marked shifted Extrude primary indexed header. The marker shifts the operation field run by one byte.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | The shifted Extrude prologue stores u32 `1` at primary-header offset 20 |
| 24 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes at offsets 24 through 26 |
| 27 | 1 | `operation_prefix_marker` | `u8` | little | spec | inserts marker byte `01` at primary-header offset 27 |
| 28 | 4 | `operation` | `u32` | little | spec | The result operation is at offset 28 |
| 32 | 4 | `direction` | `u32` | little | spec | travel direction at offset 32 |
| 36 | 4 | `face_extend` | `u32` | little | spec | face-extend option at offset 36 |
| 40 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal at offset 40 |
| 41 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind at offset 41 |
| 42 | 1 | `start_support` | `u8` | little | spec | start support at offset 42 |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding shifted-operation envelope are outside this field run.

## `legacy_class_415_symmetric_extrude_prefix`

Spec §3.1 · layout: byte offsets · size: 292 B

Offsets are relative to the class-415 primary indexed header. The primary/paired frame lengths are 447 B with five ordered references and 469 B with seven; the fixed prefix ends at the ordered reference-count field at offset 288.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | stores u32 `1` · value `1` |
| 24 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes at offsets 24 through 26 |
| 27 | 1 | `operation_prefix_marker` | `u8` | little | spec | operation prefix marker `1` · value `1` |
| 28 | 4 | `operation` | `u32` | little | spec | result operation |
| 32 | 4 | `direction` | `u32` | little | spec | travel direction `3` (symmetric) · value `3` |
| 36 | 4 | `face_extend` | `u32` | little | spec | face-extend value `2` · value `2` |
| 40 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal |
| 41 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind |
| 42 | 1 | `start_support` | `u8` | little | spec | start support |
| 43 | 3 | `zero_run_3_after_start` | `bytes[3]` | little | spec | `+43..+45` |
| 46 | 24 | `profile_normal` | `f64[3]` | little | spec | finite unit profile normal |
| 70 | 47 | `reference_slots` | `bytes[47]` | little | spec | seven nullable slots in order: absent, present, present, present, absent, present, absent |
| 117 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent `1` (distance) · value `1` |
| 121 | 9 | `zero_run_9` | `bytes[9]` | little | spec | `+121..+129` |
| 130 | 4 | `second_side_extent` | `u32` | little | spec | second-side extent `1` (distance) · value `1` |
| 288 | 4 | `reference_count` | `u32` | little | spec | ordered reference count: `5` or `7` according to frame length |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.
- `134..288` (154 B): Intervening scope bytes carry no extent discriminator and precede the ordered reference-count field.

## `legacy_class_415_one_sided_to_face_extrude_prefix`

Spec §3.1 · layout: byte offsets · size: 282 B

Offsets are relative to the class-415 primary indexed header. The paired frame begins at 481 B, the ordered reference count is at offset 278, and the fixed prefix ends after that count.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | stores u32 `1` · value `1` |
| 24 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes at offsets 24 through 26 |
| 27 | 1 | `operation_prefix_marker` | `u8` | little | spec | operation prefix marker `1` · value `1` |
| 28 | 4 | `operation` | `u32` | little | spec | result operation |
| 32 | 4 | `direction` | `u32` | little | spec | one-sided travel direction `1` · value `1` |
| 36 | 4 | `face_extend` | `u32` | little | spec | face-extend option `1` · value `1` |
| 40 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal |
| 41 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind |
| 42 | 1 | `start_support` | `u8` | little | spec | start support |
| 43 | 3 | `zero_run_3_after_start` | `bytes[3]` | little | spec | `+43..+45` |
| 46 | 24 | `profile_normal` | `f64[3]` | little | spec | finite unit profile normal |
| 107 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent `2` (to face) · value `2` |
| 111 | 11 | `first_side_offset_reference` | `bytes[11]` | little | spec | marked `Side1Offset` owner reference |
| 274 | 4 | `second_side_extent` | `u32` | little | spec | second-side extent `0` four bytes before the reference count · value `0` |
| 278 | 4 | `reference_count` | `u32` | little | spec | Ordered reference count · value `9` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.
- `70..107` (37 B): Scope-local reference lanes precede the first-side extent.
- `122..274` (152 B): The remaining scope envelope precedes the ordered reference-count field.

## `legacy_class_415_one_sided_distance_extrude_prefix`

Spec §3.1 · layout: byte offsets · size: 272 B

Offsets are relative to the class-415 primary indexed header. The paired frame begins at 449 B, the ordered reference count is at offset 268, and the fixed prefix ends after that count.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | stores u32 `1` · value `1` |
| 24 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes at offsets 24 through 26 |
| 27 | 1 | `operation_prefix_marker` | `u8` | little | spec | operation prefix marker `1` · value `1` |
| 28 | 4 | `operation` | `u32` | little | spec | result operation |
| 32 | 4 | `direction` | `u32` | little | spec | one-sided travel direction `1` · value `1` |
| 36 | 4 | `face_extend` | `u32` | little | spec | face-extend option `2` · value `2` |
| 40 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal |
| 41 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind |
| 42 | 1 | `start_support` | `u8` | little | spec | start support |
| 43 | 3 | `zero_run_3_after_start` | `bytes[3]` | little | spec | `+43..+45` |
| 46 | 24 | `profile_normal` | `f64[3]` | little | spec | finite unit profile normal |
| 107 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent `1` (distance) · value `1` |
| 264 | 4 | `second_side_extent` | `u32` | little | spec | second-side extent `0` four bytes before the reference count · value `0` |
| 268 | 4 | `reference_count` | `u32` | little | spec | Ordered reference count · value `7` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.
- `70..107` (37 B): Scope-local reference lanes precede the first-side extent.
- `111..264` (153 B): The distance form has no first-side owner lane; its second-side extent is at the end of the scope envelope.

## `shifted_extrude_offset_profile_extent_lane`

Spec §3.1 · layout: byte offsets · size: 134 B

Offsets are relative to the shifted Extrude primary indexed header. The lane ends before the remaining scope envelope.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 116 | 4 | `first_side_extent` | `u32` | little | spec | uses the widened extent lane at offsets 116 and 130 |
| 130 | 4 | `second_side_extent` | `u32` | little | spec | uses the widened extent lane at offsets 116 and 130 |

Unstated regions:

- `0..116` (116 B): The shifted prologue, profile-normal envelope, and unselected fields precede the widened extent lane.
- `120..130` (10 B): The fixed widened-lane payload separates the side extent values.

## `marked_shifted_extrude_symmetric_extent_lane`

Spec §3.1 · layout: byte offsets · size: 135 B

Offsets are relative to the marked shifted Extrude primary indexed header. Unselected fields in the intervening envelope have no extent semantics.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 117 | 4 | `first_side_extent` | `u32` | little | spec | its first-side extent `1` is at offset 117 |
| 131 | 4 | `second_side_extent` | `u32` | little | spec | its second-side extent `0` is at offset 131 |

Unstated regions:

- `0..117` (117 B): The marked shifted prologue, profile-normal envelope, and unselected extent fields precede the widened extent lane.
- `121..131` (10 B): The fixed widened-lane payload separates the side extent values.

## `shifted_extrude_offset_283_two_sided_tail`

Spec §3.1 · layout: byte offsets · size: 204 B

Offsets are relative to the shifted Extrude primary indexed header. The tail ends immediately before the following LP-UTF16 GUID field.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 139 | 11 | `first_parameter_reference` | `bytes[11]` | little | spec | with marked parameter references at offsets 139 and 170 |
| 166 | 4 | `first_side_extent` | `u32` | little | spec | the side extents at offsets 166 and 181 |
| 170 | 11 | `second_parameter_reference` | `bytes[11]` | little | spec | with marked parameter references at offsets 139 and 170 |
| 181 | 4 | `second_side_extent` | `u32` | little | spec | the side extents at offsets 166 and 181 |
| 185 | 11 | `trailing_entity_reference` | `bytes[11]` | little | spec | a trailing marked entity reference at offset 185 |
| 196 | 8 | `zero_run_8` | `bytes[8]` | little | spec | eight zero bytes at offsets 196 through 203 |

Unstated regions:

- `0..139` (139 B): The preceding shifted prologue and profile-normal envelope are outside this tail.
- `150..166` (16 B): Sixteen zero bytes separate the first parameter reference from the first-side extent.

## `thread_standard_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 38 B

Direct-prefix offsets are relative to the primary Thread indexed header. The three LP-UTF16 fields begin at offset 38 and are outside this fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The direct prefix stores ten zero bytes at offsets 11 through 20 |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes at offsets 11 through 20 |
| 21 | 8 | `fixed_scalar` | `f64` | little | spec | f64 `60.0` at offset 21 |
| 29 | 5 | `standard_marker` | `bytes[5]` | little | spec | the standard form marker is `01 02 00 00 00` |
| 34 | 4 | `standard_prefix_tail` | `bytes[4]` | little | spec | its form token is `36 00 67 00` |

## `thread_compact_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 38 B

The direct compact prefix has the same fixed width as the direct standard prefix and starts the same three LP-UTF16 fields at offset 38.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The direct prefix stores ten zero bytes at offsets 11 through 20 |
| 21 | 8 | `fixed_scalar` | `f64` | little | spec | f64 `60.0` at offset 21 |
| 29 | 5 | `compact_marker` | `bytes[5]` | little | spec | The compact form marker is `00 02 00 00 00` |
| 34 | 4 | `compact_prefix_tail` | `bytes[4]` | little | spec | its form token is `36 00 48 00` |

Unstated regions:

- `11..21` (10 B): The compact form retains the common zero run and fixed scalar before its discriminator bytes.

## `thread_owner_marked_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the primary Thread indexed header. The three LP-UTF16 fields begin at offset 42 and are outside this fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The owner-marked prefix stores nine zero bytes at offsets 11 through 19 |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | nine zero bytes at offsets 11 through 19 |
| 20 | 4 | `owner_marker` | `u32` | little | spec | u32 `1` at offset 20 |
| 24 | 1 | `separator` | `u8` | little | spec | one zero byte at offset 24 |
| 25 | 8 | `fixed_scalar` | `f64` | little | spec | f64 `60.0` at offset 25 |
| 33 | 5 | `form_marker` | `bytes[5]` | little | spec | Its form marker starts at offset 33 |
| 38 | 4 | `form_token` | `bytes[4]` | little | spec | its form token starts at offset 38 |

## `thread_standard_construction_tail`

Spec §3.1 · layout: byte offsets · size: 40 B

Offsets are relative to the first byte after the third LP-UTF16 string.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `construction_marker` | `bytes[5]` | little | spec | the five-byte construction marker is `00 01 00 00 00` |
| 5 | 8 | `major_diameter` | `f64` | little | spec | Major diameter is at marker-relative offset 5 |
| 13 | 8 | `minor_diameter` | `f64` | little | spec | minor diameter at 13 |
| 21 | 1 | `pitch_marker` | `u8` | little | spec | the pitch marker at 21 |
| 22 | 8 | `pitch` | `f64` | little | spec | pitch at 22 |
| 30 | 8 | `pitch_diameter` | `f64` | little | spec | pitch diameter at 30 |
| 38 | 2 | `standard_trailer` | `bytes[2]` | little | spec | The standard trailer at relative offset 38 is `00 01` |

## `thread_compact_construction_tail`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the first byte after the third LP-UTF16 string.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `construction_marker` | `bytes[5]` | little | spec | `01 02 00 00 00` in the compact form |
| 38 | 4 | `compact_trailer` | `bytes[4]` | little | spec | the compact trailer is `00 00 00 01` |

Unstated regions:

- `5..38` (33 B): The compact scalar lanes use the same offsets as the standard construction tail.

## `edge_flange_fixed_operation_section`

Spec §3.1 · layout: byte offsets · size: 79 B

Offsets are relative to the section base `85 + S`, where `S` is the header shift. The section runs from the bend-position discriminator through the inside bend radius; the result-record run and the two closing group references follow it at variable offsets.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `bend_position` | `u32` | little | spec | with a u32 bend-position discriminator |
| 4 | 4 | `edge_count` | `u32` | little | spec | the u32 edge count `1` |
| 8 | 11 | `edge_wrapper_reference` | `bytes[11]` | little | spec | the marked wrapper reference |
| 19 | 11 | `settings_reference` | `bytes[11]` | little | spec | the marked settings reference |
| 30 | 4 | `height_datum` | `u32` | little | spec | a u32 height-datum discriminator |
| 34 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | and the marked angle-owner and height-owner references |
| 45 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | Height-datum value `1` measures the height from the inner faces |
| 56 | 4 | `unsettled_side_reference` | `u32` | little | spec | A u32 whose role is not settled follows the height-owner reference |
| 71 | 8 | `inside_bend_radius` | `f64` | little | spec | the positive f64 inside bend radius in centimetres starts 15 bytes after that u32 |

Unstated regions:

- `60..71` (11 B): The spec places the radius 15 bytes after the unsettled u32, so eleven bytes between them are unaccounted for.

## `edge_flange_legacy_single_edge_fixed_operation`

Spec §3.1 · layout: byte offsets · size: 218 B

Offsets are relative to the primary scope header. The class-325/class-258 and class-334/class-257 forms have a 494-byte primary frame; the fixed operation fields close at the marked role-0x08 group reference.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 76 | 4 | `bend_position` | `u32` | little | spec | the fixed fields are bend position at offset |
| 80 | 4 | `edge_count` | `u32` | little | spec | edge count `1` at |
| 84 | 11 | `edge_wrapper_reference` | `bytes[11]` | little | spec | the marked wrapper reference at |
| 95 | 11 | `settings_reference` | `bytes[11]` | little | spec | the marked settings reference at |
| 106 | 4 | `height_datum` | `u32` | little | spec | height datum at |
| 110 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | the marked angle owner at |
| 121 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | the marked height owner at |
| 132 | 4 | `reference_side` | `u32` | little | spec | reference-side discriminator at |
| 138 | 8 | `inside_bend_radius` | `f64` | little | spec | the positive finite f64 inside bend radius in centimetres at |
| 146 | 4 | `result_count` | `u32` | little | spec | The result count at |
| 150 | 11 | `result_one_reference` | `bytes[11]` | little | spec | the two 15-byte result records start at |
| 161 | 4 | `result_one_trailer` | `u32` | little | spec | have u32 trailers `1` at |
| 165 | 11 | `result_two_reference` | `bytes[11]` | little | spec | and `165`, carry marked references |
| 176 | 4 | `result_two_trailer` | `u32` | little | spec | and `0` at |
| 180 | 4 | `result_separator` | `u32` | little | spec | A u32 value `1` at |
| 184 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | precedes the marked aggregate-group reference at |
| 207 | 11 | `edge_group_reference` | `bytes[11]` | little | spec | the marked role-`0x08` group reference starts at |

Unstated regions:

- `0..76` (76 B): The indexed header and the variable scope envelope precede the fixed operation section.
- `136..138` (2 B): Two bytes separate the side discriminator from the radius.
- `195..207` (12 B): Twelve zero bytes separate the aggregate-group reference from the edge-group reference.

## `edge_flange_multi_edge_fixed_operation`

Spec §3.1 · layout: byte offsets · size: 271 B

Offsets are relative to the primary scope header. The fixed operation fields close at the second marked role-0x08 group reference; the paired header follows the 591-byte primary frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 92 | 4 | `bend_position` | `u32` | little | spec | the bend-position discriminator is at offset `92` |
| 96 | 4 | `edge_count` | `u32` | little | spec | the edge count at `96` |
| 100 | 11 | `edge_wrapper_one_reference` | `bytes[11]` | little | spec | the two marked wrapper references start at |
| 111 | 11 | `edge_wrapper_two_reference` | `bytes[11]` | little | spec | and `111`, the marked settings reference |
| 122 | 11 | `settings_reference` | `bytes[11]` | little | spec | the marked settings reference at |
| 133 | 4 | `height_datum` | `u32` | little | spec | height datum at |
| 137 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | the marked angle owner at |
| 148 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | the marked height owner at |
| 159 | 4 | `reference_side` | `u32` | little | spec | reference-side discriminator at |
| 165 | 8 | `inside_bend_radius` | `f64` | little | spec | the positive finite f64 inside bend radius in centimetres at |
| 173 | 4 | `result_count` | `u32` | little | spec | The result count at |
| 177 | 11 | `result_one_reference` | `bytes[11]` | little | spec | the three 15-byte result records start at |
| 188 | 4 | `result_one_trailer` | `u32` | little | spec | have u32 trailers `1` at |
| 192 | 11 | `result_two_reference` | `bytes[11]` | little | spec | records start at `177`, `192`, and `207` |
| 203 | 4 | `result_two_trailer` | `u32` | little | spec | `1` at `203` |
| 207 | 11 | `result_three_reference` | `bytes[11]` | little | spec | and `207`, and have u32 trailers |
| 218 | 4 | `result_three_trailer` | `u32` | little | spec | and `0` at |
| 222 | 4 | `result_separator` | `u32` | little | spec | A u32 value `1` at |
| 226 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | precedes the marked aggregate-group reference at |
| 249 | 11 | `edge_group_one_reference` | `bytes[11]` | little | spec | The two marked role-`0x08` group references start at |
| 260 | 11 | `edge_group_two_reference` | `bytes[11]` | little | spec | and `260`, and each group's recipe-backed operand |

Unstated regions:

- `0..92` (92 B): The indexed header and variable scope envelope precede the fixed operation section.
- `163..165` (2 B): Two bytes separate the side discriminator from the radius.
- `237..249` (12 B): Twelve zero bytes separate the aggregate-group reference from the first edge-group reference.

## `edge_flange_class325_334_two_sided_per_edge_fixed_operation`

Spec §3.1 · layout: byte offsets · size: 305 B

Offsets are relative to the primary scope header. The fixed operation fields close at the second marked role-0x08 group reference; the paired header follows the 669-byte primary frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 96 | 4 | `bend_position` | `u32` | little | spec | bend position at `96` |
| 100 | 4 | `edge_count` | `u32` | little | spec | edge count `2` at `100` |
| 104 | 11 | `edge_wrapper_one_reference` | `bytes[11]` | little | spec | wrapper references at `104` and `115` |
| 115 | 11 | `edge_wrapper_two_reference` | `bytes[11]` | little | spec | and `115`, settings at `126` |
| 126 | 11 | `settings_reference` | `bytes[11]` | little | spec | settings at `126` |
| 137 | 4 | `height_datum` | `u32` | little | spec | height datum at `137` |
| 141 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | angle owner at `141` |
| 152 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | height owner at `152` |
| 163 | 4 | `reference_side` | `u32` | little | spec | reference side at `163` |
| 169 | 8 | `inside_bend_radius` | `f64` | little | spec | inside bend radius in centimetres at `169` |
| 177 | 4 | `result_count` | `u32` | little | spec | Result count `5` is at `177` |
| 181 | 11 | `result_one_reference` | `bytes[11]` | little | spec | result records start at `181` |
| 192 | 4 | `result_one_trailer` | `u32` | little | spec | trailers `1` at `192` |
| 196 | 11 | `result_two_reference` | `bytes[11]` | little | spec | `196`, `211`, `226`, and `241` |
| 207 | 4 | `result_two_trailer` | `u32` | little | spec | `207`, `222`, `237`, and `0` |
| 211 | 11 | `result_three_reference` | `bytes[11]` | little | spec | `196`, `211`, `226`, and `241` |
| 222 | 4 | `result_three_trailer` | `u32` | little | spec | `222`, `237`, and `0` |
| 226 | 11 | `result_four_reference` | `bytes[11]` | little | spec | `196`, `211`, `226`, and `241` |
| 237 | 4 | `result_four_trailer` | `u32` | little | spec | `237`, and `0` |
| 241 | 11 | `result_five_reference` | `bytes[11]` | little | spec | `196`, `211`, `226`, and `241` |
| 252 | 4 | `result_five_trailer` | `u32` | little | spec | and `0` at `252` |
| 256 | 4 | `result_separator` | `u32` | little | spec | A u32 value `1` at `256` |
| 260 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | aggregate-group reference at `260` |
| 283 | 11 | `edge_group_one_reference` | `bytes[11]` | little | spec | group references are at `283` and `294` |
| 294 | 11 | `edge_group_two_reference` | `bytes[11]` | little | spec | at `283` and `294` |

Unstated regions:

- `0..96` (96 B): The indexed header and variable scope envelope precede the fixed operation section.
- `167..169` (2 B): Two bytes separate the side discriminator from the radius.
- `271..283` (12 B): Twelve bytes separate the aggregate-group reference from the first edge-group reference.

## `edge_flange_class364_per_edge_width_fixed_operation`

Spec §3.1 · layout: byte offsets · size: 301 B

Offsets are relative to the primary scope header. The fixed operation fields close at the second marked role-0x08 group reference; the paired header follows the 643-byte primary frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 92 | 4 | `bend_position` | `u32` | little | spec | The bend-position discriminator is at `92` |
| 96 | 4 | `edge_count` | `u32` | little | spec | the edge count `2` at `96` |
| 100 | 11 | `edge_wrapper_one_reference` | `bytes[11]` | little | spec | the marked wrapper references at `100` and `111` |
| 111 | 11 | `edge_wrapper_two_reference` | `bytes[11]` | little | spec | and `111`, the marked settings reference at `122` |
| 122 | 11 | `settings_reference` | `bytes[11]` | little | spec | the marked settings reference at `122` |
| 133 | 4 | `height_datum` | `u32` | little | spec | height datum at `133` |
| 137 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | the marked angle owner at `137` |
| 148 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | the marked height owner at `148` |
| 159 | 4 | `reference_side` | `u32` | little | spec | reference-side discriminator at `159` |
| 165 | 8 | `inside_bend_radius` | `f64` | little | spec | the positive finite f64 inside bend radius in centimetres at `165` |
| 173 | 4 | `result_count` | `u32` | little | spec | The result count at `173` is `5` |
| 177 | 11 | `result_one_reference` | `bytes[11]` | little | spec | the five 15-byte result records start at `177` |
| 188 | 4 | `result_one_trailer` | `u32` | little | spec | have u32 trailers `1` at `188` |
| 192 | 11 | `result_two_reference` | `bytes[11]` | little | spec | `192`, `207`, `222`, and `237` |
| 203 | 4 | `result_two_trailer` | `u32` | little | spec | `1` at `203` |
| 207 | 11 | `result_three_reference` | `bytes[11]` | little | spec | `192`, `207`, `222`, and `237` |
| 218 | 4 | `result_three_trailer` | `u32` | little | spec | trailers `1` at `188`, `203`, `218`, `233` |
| 222 | 11 | `result_four_reference` | `bytes[11]` | little | spec | `192`, `207`, `222`, and `237` |
| 233 | 4 | `result_four_trailer` | `u32` | little | spec | `218`, `233`, and `0` at `248` |
| 237 | 11 | `result_five_reference` | `bytes[11]` | little | spec | `192`, `207`, `222`, and `237` |
| 248 | 4 | `result_five_trailer` | `u32` | little | spec | and `0` at `248` |
| 252 | 4 | `result_separator` | `u32` | little | spec | A u32 value `1` at `252` |
| 256 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | the marked aggregate-group reference at `256` |
| 279 | 11 | `edge_group_one_reference` | `bytes[11]` | little | spec | The two marked role-`0x08` group references start at `279` |
| 290 | 11 | `edge_group_two_reference` | `bytes[11]` | little | spec | and `290`, and each group's recipe-backed operand |

Unstated regions:

- `0..92` (92 B): The indexed header and variable scope envelope precede the fixed operation section.
- `163..165` (2 B): Two bytes separate the side discriminator from the radius.
- `267..279` (12 B): Twelve zero bytes separate the aggregate-group reference from the first edge-group reference.

## `edge_flange_class286_single_edge_fixed_operation`

Spec §3.1 · layout: byte offsets · size: 207 B

Offsets are relative to the primary scope header. The fixed operation fields close at the marked role-0x08 group reference; the paired header follows the 483-byte primary frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 80 | 4 | `bend_position` | `u32` | little | spec | bend position at offset `80` |
| 84 | 4 | `edge_count` | `u32` | little | spec | edge count `1` at `84` |
| 88 | 11 | `edge_wrapper_reference` | `bytes[11]` | little | spec | the marked wrapper reference at `88` |
| 99 | 11 | `settings_reference` | `bytes[11]` | little | spec | the marked settings reference at `99` |
| 110 | 4 | `height_datum` | `u32` | little | spec | height datum at `110` |
| 114 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | the marked angle owner at `114` |
| 125 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | the marked height owner at `125` |
| 136 | 4 | `reference_side` | `u32` | little | spec | reference-side discriminator at `136` |
| 142 | 8 | `inside_bend_radius` | `f64` | little | spec | the positive finite f64 inside bend radius in centimetres at `142` |
| 150 | 4 | `result_count` | `u32` | little | spec | The result count at `150` |
| 154 | 11 | `result_reference` | `bytes[11]` | little | spec | its 15-byte result record starts at `154` |
| 165 | 4 | `result_trailer` | `u32` | little | spec | a u32 trailer `0` at `165` |
| 169 | 4 | `result_separator` | `u32` | little | spec | A u32 value `1` at `169` |
| 173 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | the marked aggregate-group reference at `173` |
| 196 | 11 | `edge_group_reference` | `bytes[11]` | little | spec | the marked role-`0x08` group reference starts at `196` |

Unstated regions:

- `0..80` (80 B): The indexed header and variable scope envelope precede the fixed operation section.
- `140..142` (2 B): Two bytes separate the side discriminator from the radius.
- `184..196` (12 B): Twelve zero bytes separate the aggregate-group reference from the edge-group reference.

## `edge_flange_to_object_fixed_operation_section`

Spec §3.1 · layout: byte offsets · size: 181 B

Offsets are relative to the section base `85 + S`. This single-edge form closes at the marked role-`0x08` group reference; the paired header follows at `576 + S`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 94 | 11 | `target_group_reference` | `bytes[11]` | little | spec | the marked target-group reference starts at |
| 105 | 4 | `target_reference_count` | `u32` | little | spec | The u32 at `105 + S` is `2` |
| 109 | 11 | `inserted_reference_one` | `bytes[11]` | little | spec | marked first inserted reference starts at |
| 120 | 4 | `inserted_reference_count` | `u32` | little | spec | the u32 at `120 + S` is `1` |
| 124 | 11 | `inserted_reference_two` | `bytes[11]` | little | spec | marked second inserted reference starts at |
| 139 | 4 | `aggregate_reference_count` | `u32` | little | spec | the u32 at `139 + S` is `1` |
| 143 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | marked aggregate-group reference starts at |
| 166 | 4 | `edge_reference_count` | `u32` | little | spec | the u32 at `166 + S` is `1` |
| 170 | 11 | `edge_group_reference` | `bytes[11]` | little | spec | marked role-`0x08` group reference starts at |

Unstated regions:

- `0..94` (94 B): The common operation fields through the single result-record count occupy offsets 0 through 93.
- `135..139` (4 B): Four zero bytes precede the structural u32 value `1` at offset 139.
- `154..166` (12 B): Twelve zero bytes precede the structural u32 value `1` at offset 166.

## `hem_gap_length_fixed_operation_section`

Spec §3.1 · layout: byte offsets · size: 79 B

Offsets are relative to the section base `85 + S`, where `S` is the header shift. The aggregate and role-`0x08` group references follow this fixed section at offsets 108 and 135.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 4 | 4 | `edge_count` | `u32` | little | spec | the u32 edge count `1` at offset |
| 8 | 11 | `edge_wrapper_reference` | `bytes[11]` | little | spec | the marked edge-wrapper reference at |
| 19 | 11 | `settings_reference` | `bytes[11]` | little | spec | the marked settings reference at |
| 42 | 11 | `gap_owner_reference` | `bytes[11]` | little | spec | the marked gap-owner |
| 53 | 11 | `length_owner_reference` | `bytes[11]` | little | spec | length-owner references at |
| 71 | 8 | `inside_bend_radius` | `f64` | little | spec | a positive f64 rule-derived inside bend radius in centimetres at |

Unstated regions:

- `0..4` (4 B): The fixed section begins with a retained u32 outside the typed owner and radius fields.
- `30..42` (12 B): The fixed section retains the direction, reversal, and reference-side fields between the settings and owner references.
- `64..71` (7 B): The fixed section has seven bytes between the length-owner reference and the rule-derived radius.

## `hem_rolled_fixed_operation_section`

Spec §3.1 · layout: byte offsets · size: 79 B

Offsets are relative to the section base `85 + S`. The angle owner precedes the radius owner in the fixed section; source kinds assign their semantic roles.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 41 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | the first owner is the angle input |
| 54 | 11 | `radius_owner_reference` | `bytes[11]` | little | spec | the second is the radius input |
| 71 | 8 | `inside_bend_radius` | `f64` | little | spec | its rule-derived inside bend radius remains at |

Unstated regions:

- `0..41` (41 B): The fixed section begins with the retained fields and common envelope before the angle-owner reference.
- `52..54` (2 B): The rolled owner references are separated by two bytes in the fixed section.
- `65..71` (6 B): The fixed section has six bytes between the radius-owner reference and the rule-derived radius.

## `hem_teardrop_fixed_operation_section`

Spec §3.1 · layout: byte offsets · size: 89 B

Offsets are relative to the section base `85 + S`; the aggregate and role-`0x08` group references lie at offsets 118 and 145 after the third owner slot.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 42 | 11 | `gap_owner_reference` | `bytes[11]` | little | spec | marked gap-owner |
| 53 | 11 | `length_owner_reference` | `bytes[11]` | little | spec | length-owner |
| 64 | 11 | `radius_owner_reference` | `bytes[11]` | little | spec | radius-owner references |
| 81 | 8 | `inside_bend_radius` | `f64` | little | spec | its rule-derived inside bend radius starts |

Unstated regions:

- `0..42` (42 B): The fixed section begins with the retained fields and common envelope before the gap-owner reference.
- `75..81` (6 B): The fixed section has six bytes between the radius-owner reference and the rule-derived radius.

## `move_transform_frame_253`

Spec §3.1 · layout: byte offsets · size: 253 B

Offsets are relative to the transform record's primary indexed header. The class tags are the admission discriminator; the class-447 form uses paired class 263; the same-index paired header follows at offset 253.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | transform records are 253 bytes to their same-index paired headers |
| 43 | 4 | `form` | `u32` | little | spec | offset 43 stores u32 form `1` or `5` |
| 47 | 1 | `reserved_zero` | `u8` | little | spec | offset 47 is zero |
| 48 | 128 | `transform` | `f64[16]` | little | spec | sixteen row-major f64 values begin at offset 48 |

Unstated regions:

- `11..43` (32 B): Offsets 11 through 42 are zero.
- `176..253` (77 B): The transform record's native tail precedes the same-index paired header at offset 253.

## `legacy_body_group_frame_123`

Spec §3.1 · layout: byte offsets · size: 123 B

Offsets are relative to the primary indexed header for the ordinary one-member, two-null-auxiliary, one-trailing-reference envelope. Primary/paired classes are 257/262, 323/262, 328/263, 338/261, 282/262, and 302/258; the tail discriminants are 01 01, 01 01, 01 01, 01 01, 00 01, and 00 01 respectively. The class-328 Move variant remains 123 bytes but uses a null auxiliary reference at +36, a present auxiliary reference to N+13 at +37, trailing count zero at +48, and a retained null trailing-slot byte at +52 before the role at +53; its tail has byte 0 at +98, discriminant 01 01 at +99, an unmarked N+1 reference at +101, and the owning-scope reference at +112. It has no counted trailing-reference run.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | `byte_offset` anchors the primary indexed header |
| 21 | 4 | `member_count` | `u32` | little | spec | u32 member_count |
| 25 | 11 | `member_reference` | `bytes[11]` | little | spec | the member reference run |
| 53 | 8 | `role` | `u64` | little | spec | a u64 role with low word zero |
| 71 | 4 | `opaque_ordinal` | `u32` | little | spec | u32 opaque_ordinal |
| 75 | 8 | `opaque_scalar` | `f64` | little | spec | one finite f64 opaque scalar |
| 83 | 4 | `repeated_ordinal` | `u32` | little | spec | a second copy of `opaque_ordinal` |
| 87 | 11 | `n_plus_2_reference` | `bytes[11]` | little | spec | a reference to `N+2` |
| 98 | 2 | `tail_discriminant` | `bytes[2]` | little | spec | a two-byte discriminant |
| 100 | 11 | `n_plus_1_reference` | `bytes[11]` | little | spec | a reference to `N+1` |
| 111 | 1 | `tail_zero` | `u8` | little | spec | byte `0` |
| 112 | 11 | `owning_scope_reference` | `bytes[11]` | little | spec | a reference to the owning scope |

Unstated regions:

- `11..21` (10 B): The common prefix has ten zero bytes after the indexed header.
- `36..53` (17 B): The one-member envelope has two null auxiliary references, a one-entry trailing count, and its trailing reference.
- `61..71` (10 B): Ten zero bytes follow the role.

## `component_insert_identity_scope_296_263`

Spec §3.1 · layout: byte offsets · size: 261 B

Offsets are relative to the primary class-296 indexed header. The paired class-263 indexed header begins at offset 261.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Relative to its primary indexed header |
| 20 | 1 | `prologue_marker` | `u8` | little | spec | `01 00 00 00 00` at offsets 20 through 24 |
| 21 | 4 | `prologue_value` | `u32` | little | spec | `01 00 00 00 00` at offsets 20 through 24 |
| 25 | 8 | `occurrence_identity` | `u64` | little | spec | an occurrence identity u64 at offset 25 |
| 37 | 1 | `relation_marker` | `u8` | little | spec | a marked reference to the sole relation at offset 37 |
| 38 | 4 | `relation_record_index` | `u32` | little | spec | a marked reference to the sole relation at offset 37 |
| 48 | 2 | `identity_markers` | `bytes[2]` | little | spec | bytes `01 01` at offsets 48 and 49 |
| 50 | 4 | `opaque_code_unit_count` | `u32` | little | spec | u32 code-unit count 36 at offset 50 |
| 54 | 72 | `opaque_utf16_payload` | `bytes[72]` | little | spec | the 36-code-unit null GUID `00000000-0000-0000-0000-000000000000` at offset 54 |

Unstated regions:

- `11..20` (9 B): Nine zero bytes occupy offsets 11 through 19.
- `33..37` (4 B): Four zero bytes occupy offsets 33 through 36.
- `42..48` (6 B): Six zero bytes occupy offsets 42 through 47.
- `126..261` (135 B): The feature-family tail, ordered reference table, state fields, and paired-header backlink occupy the remaining fixed frame.

## `component_insert_grouped_identity_carrier_382`

Spec §3.1 · layout: byte offsets · size: 695 B

Offsets are relative to the primary class-382 grouped identity carrier header. The relation header begins at offset 695. Every GUID field has 36 code units or ASCII bytes.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 1 | `carrier_marker` | `u8` | little | spec | byte `1` at offset 19 |
| 20 | 4 | `carrier_value` | `u32` | little | spec | u32 `1` at offset 20 |
| 24 | 1 | `identity_marker` | `u8` | little | spec | byte `1` at offset 24 |
| 25 | 8 | `occurrence_identity` | `u64` | little | spec | an occurrence identity u64 at offset 25 |
| 33 | 1 | `reference_marker` | `u8` | little | spec | byte `1` at offset 33 |
| 38 | 76 | `first_component_guid` | `bytes[76]` | little | spec | At offset 38 it stores an LP-UTF16 component GUID |
| 114 | 1 | `first_component_separator` | `u8` | little | spec | one zero separator byte |
| 115 | 40 | `first_type_guid` | `bytes[40]` | little | spec | an LP-ASCII type GUID |
| 155 | 76 | `first_role_guid` | `bytes[76]` | little | spec | an LP-UTF16 occurrence-role GUID |
| 231 | 10 | `metadata_marker` | `bytes[10]` | little | spec | The role is followed by `00 01 00 00 00 00 01 00 00 00` |
| 241 | 76 | `metadata_guid_a` | `bytes[76]` | little | spec | two LP-UTF16 metadata GUIDs |
| 317 | 76 | `metadata_guid_b` | `bytes[76]` | little | spec | two LP-UTF16 metadata GUIDs |
| 393 | 15 | `placement_marker` | `bytes[15]` | little | spec | `00 01 03 00 00 00 00 00 00 00 01 00 00 00 00` |
| 408 | 76 | `repeated_component_guid` | `bytes[76]` | little | spec | It then repeats the component GUID |
| 484 | 1 | `repeated_component_separator` | `u8` | little | spec | separator byte |
| 485 | 40 | `repeated_type_guid` | `bytes[40]` | little | spec | type GUID, and role GUID |
| 525 | 76 | `repeated_role_guid` | `bytes[76]` | little | spec | type GUID, and role GUID |
| 601 | 6 | `construction_marker` | `bytes[6]` | little | spec | followed by `00 01 00 00 00 00` |
| 607 | 76 | `final_role_guid` | `bytes[76]` | little | spec | a final role GUID |
| 683 | 12 | `closure` | `bytes[12]` | little | spec | `00 01 04 00 00 00 00 00 00 00 00 00` |

Unstated regions:

- `0..11` (11 B): The indexed carrier header occupies the first eleven bytes.
- `11..19` (8 B): Eight zero bytes occupy offsets 11 through 18.
- `34..38` (4 B): Four zero bytes occupy offsets 34 through 37.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Container, manifest, and stream-selection layers (§1, §2) | §1.3 | ZIP entries and `Manifest.dat` text grammar; no fixed byte-offset structure is stated. |
