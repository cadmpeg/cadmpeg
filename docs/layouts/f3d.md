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
(§3.1), plus the `Canvas` geometry and scope records, the `Decal` scope, image-record prefixes, current sketch-container visibility member, the grouped identity `Component Insert` frames, the class-`283` / class-`262` `Component Insert` scope frames with their class-`334` carrier prefix, and the local component-occurrence frames and root-instance prefix. ASM stream records are tabulated in `docs/layouts/asm.toml`. Protein page records are tabulated in `docs/layouts/protein.toml`.
Container and manifest layers are text grammars and are listed under "Not
tabulated".

## Composite types

| Type | Bytes | Endianness | Meaning |
| ---- | ----: | ---------- | ------- |
| `component_body_membership_count` | 4 | little | The u32 count precedes the terminal Body-reference list. |
| `component_body_membership_reference` | 51 | little | A present same-segment reference with a nonzero u64 target, a 36-byte ASCII type GUID, and two trailing zero bytes. |
| `component_body_membership_local_reference` | 11 | little | A present untyped same-segment reference with a nonzero u64 target and two zero bytes. |
| `component_body_membership_local_entry` | 15 | little | An 11-byte untyped Body reference followed by u32 1 in the repeated body list. List order is independent; both lists contain the same unique body identities. |
| `component_body_membership_single_overhead` | 15 | little | A u32 body count and an eleven-byte untyped owner reference enclose the single body list. |

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

## `design_parameter_typed_compact_owned_prefix`

Spec §3.1 · layout: byte offsets · size: 85 B

Offsets are relative to the indexed header of an owned typed compact parameter through its expression length. The frame ends with the 52-byte typed parameter tail.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The indexed header is followed by fifteen zero bytes |
| 11 | 15 | `zero_run_15` | `bytes[15]` | little | spec | The indexed header is followed by fifteen zero bytes |
| 26 | 4 | `source_ordinal` | `u32` | little | spec | `u32 source_ordinal` at offset 26 |
| 30 | 1 | `owner_marker` | `u8` | little | spec | The owner field at offset 30 is a reference · value `1` |
| 31 | 8 | `owner_record_index` | `u64` | little | spec | the u64 owner record index at offset 31 |
| 39 | 4 | `owner_type_guid_length` | `u32` | little | spec | its u32 length `36` at offset 39 · value `36` |
| 43 | 36 | `owner_type_guid` | `bytes[36]` | little | spec | the LP-ASCII 36 target type GUID |
| 79 | 2 | `owner_flags` | `bytes[2]` | little | spec | the two zero flag bytes at offsets 79 and 80 |
| 81 | 4 | `expression_length` | `u32` | little | spec | Its LP-UTF16 expression begins at offset 81 |

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

## `shell_class_369_261_scope_frame`

Spec §3.1 · layout: byte offsets · size: 271 B

Offsets are relative to the primary indexed header. The paired class-261 header begins at offset 271; the class pair and frame length admit this form.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | primary indexed header |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | offsets 11 through 19 are zero |
| 20 | 1 | `feature_form` | `u8` | little | spec | offset 20 is `1` · value `1` |
| 21 | 1 | `outward` | `u8` | little | spec | offset 21 is the outward Boolean |
| 22 | 3 | `zero_run_3` | `bytes[3]` | little | spec | offsets 22 through 24 are zero |
| 25 | 1 | `scalar_marker` | `u8` | little | spec | Offset 25 is `1` · value `1` |
| 26 | 11 | `scalar_reference` | `bytes[11]` | little | spec | the marked reference at offset 26 names the first ordered scalar |
| 37 | 9 | `zero_run_9_after_scalar` | `bytes[9]` | little | spec | Offsets 37 through 45 are zero |
| 46 | 1 | `group_form` | `u8` | little | spec | offset 46 is `1` · value `1` |
| 50 | 11 | `group_reference` | `bytes[11]` | little | spec | the marked reference at offset 50 names the second ordered construction group |
| 61 | 4 | `guid_code_unit_count` | `u32` | little | spec | A u32 `36` at offset 61 · value `36` |
| 65 | 72 | `guid` | `bytes[72]` | little | spec | a 36-code-unit null GUID at offset 65 |
| 137 | 3 | `zero_run_3_before_references` | `bytes[3]` | little | spec | offsets 137 through 139 are zero |
| 140 | 4 | `reference_count` | `u32` | little | spec | The three-entry ordered reference table begins at offset 140 · value `3` |
| 144 | 11 | `reference_entry_0` | `bytes[11]` | little | spec | The three-entry ordered reference table begins at offset 140 |
| 155 | 11 | `reference_entry_1` | `bytes[11]` | little | spec | The three-entry ordered reference table begins at offset 140 |
| 166 | 11 | `reference_entry_2` | `bytes[11]` | little | spec | The three-entry ordered reference table begins at offset 140 |
| 177 | 4 | `history_state_id` | `u32` | little | spec | The current history-state identity is at offset 177 |
| 181 | 4 | `kind_code_unit_count` | `u32` | little | spec | the kind count is u32 `5` at offset 181 · value `5` |
| 185 | 10 | `kind` | `bytes[10]` | little | spec | `Shell` begins at offset 185 |
| 195 | 4 | `feature_ordinal` | `u32` | little | spec | the feature ordinal follows at offset 195 |
| 225 | 4 | `previous_history_state_id` | `u32` | little | spec | the preceding history-state identity is at offset 225 |

Unstated regions:

- `47..50` (3 B): The three bytes before the marked second-group reference are zero.
- `199..225` (26 B): The generic fixed scope tail precedes the preceding history-state identity.
- `229..271` (42 B): The generic fixed scope tail closes immediately before the paired class-261 header.

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

## `design_mirror_scope_class369_tail`

Spec §3.1 · layout: byte offsets · size: 77 B

Offsets are relative to the first byte after the UTF-16LE Mirror kind. The variable class-369 reference-table prefix precedes this fixed tail.

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

- `4..31` (27 B): The class-369 tail reserves the span before the preceding-history state.

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

## `design_mirror_scope_class440_tail`

Spec §3.1 · layout: byte offsets · size: 77 B

Offsets are relative to the first byte after the UTF-16LE Mirror kind. The variable class-440 reference-table prefix precedes this fixed tail.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `feature_ordinal` | `u32` | little | spec | offset 0 stores the feature ordinal |
| 31 | 4 | `previous_history_state` | `u32` | little | spec | offset 31 stores the preceding-history state |
| 35 | 4 | `scalar_marker` | `u32` | little | spec | both scalar markers are u32 `100` · value `100` |
| 39 | 8 | `stitch_tolerance` | `f64` | little | spec | The f64 at tail offset 39 is the positive stitch tolerance in source centimetres |
| 47 | 4 | `repeated_scalar_marker` | `u32` | little | spec | both scalar markers are u32 `100` · value `100` |
| 51 | 13 | `first_reference` | `bytes[13]` | little | spec | the references at offsets 51 and 64 name scope index plus two and plus one |
| 64 | 13 | `second_reference` | `bytes[13]` | little | spec | the references at offsets 51 and 64 name scope index plus two and plus one |

Unstated regions:

- `4..31` (27 B): The class-440 tail reserves the span before the preceding-history state.

## `design_mirror_scope_class441_tail`

Spec §3.1 · layout: byte offsets · size: 72 B

Offsets are relative to the first byte after the UTF-16LE Mirror kind. The variable class-441 reference-table prefix precedes this fixed tail.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `feature_ordinal` | `u32` | little | spec | offset 0 stores the feature ordinal |
| 30 | 4 | `previous_history_state` | `u32` | little | spec | offset 30 stores the preceding-history state |
| 34 | 4 | `scalar_marker` | `u32` | little | spec | offset 34 stores u32 marker `61` · value `61` |
| 38 | 8 | `stitch_tolerance` | `f64` | little | spec | offset 38 stores the positive stitch tolerance as an f64 in source centimetres |
| 46 | 13 | `first_reference` | `bytes[13]` | little | spec | Offset 46 stores a marked reference to scope index plus two followed by two zero bytes |
| 59 | 13 | `second_reference` | `bytes[13]` | little | spec | offset 59 stores a marked reference to scope index plus one followed by two zero bytes |

Unstated regions:

- `4..30` (26 B): The class-441 tail reserves the span before the preceding-history state.

## `design_mirror_scope_class391_tail`

Spec §3.1 · layout: byte offsets · size: 76 B

Offsets are relative to the first byte after the UTF-16LE Mirror kind. The variable class-391 reference-table prefix precedes this fixed tail.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `feature_ordinal` | `u32` | little | spec | offset 0 stores the feature ordinal |
| 30 | 4 | `previous_history_state` | `u32` | little | spec | offset 30 stores the preceding-history state |
| 34 | 4 | `scalar_marker` | `u32` | little | spec | offset 34 stores u32 marker `94` · value `94` |
| 38 | 8 | `stitch_tolerance` | `f64` | little | spec | offset 38 stores the positive stitch tolerance as an f64 in source centimetres |
| 46 | 4 | `repeated_scalar_marker` | `u32` | little | spec | Offset 46 stores u32 marker `94` · value `94` |
| 50 | 13 | `first_reference` | `bytes[13]` | little | spec | offset 50 stores a marked reference to scope index plus two followed by two zero bytes |
| 63 | 13 | `second_reference` | `bytes[13]` | little | spec | offset 63 stores a marked reference to scope index plus one followed by two zero bytes |

Unstated regions:

- `4..30` (26 B): The class-391 tail reserves the span before the preceding-history state.

## `design_mirror_scope_class441_count_owner`

Spec §3.1 · layout: byte offsets · size: 99 B

Offsets are relative to the class-426 primary indexed header. The paired class-267 indexed header begins at offset 99.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | its 99-byte frame stores the exact count `2` at offset 40 |
| 19 | 1 | `one_marker` | `u8` | little | spec | its 99-byte frame stores the exact count `2` at offset 40 · value `1` |
| 20 | 4 | `one_value` | `u32` | little | spec | its 99-byte frame stores the exact count `2` at offset 40 · value `1` |
| 24 | 1 | `scope_marker` | `u8` | little | spec | its 99-byte frame stores the exact count `2` at offset 40 · value `1` |
| 25 | 4 | `scope_record_index` | `u32` | little | spec | its 99-byte frame stores the exact count `2` at offset 40 |
| 35 | 4 | `local_ordinal` | `u32` | little | spec | owned ordinal `1` at offset 55 · value `0` |
| 39 | 1 | `scalar_marker` | `u8` | little | spec | its 99-byte frame stores the exact count `2` at offset 40 · value `0` |
| 40 | 4 | `count` | `u32` | little | spec | its 99-byte frame stores the exact count `2` at offset 40 · value `2` |
| 44 | 1 | `parameter_marker` | `u8` | little | spec | a parameter reference to count-record index plus two at offset 45 · value `1` |
| 45 | 4 | `parameter_record_index` | `u32` | little | spec | a parameter reference to count-record index plus two at offset 45 |
| 55 | 4 | `owned_ordinal` | `u32` | little | spec | owned ordinal `1` at offset 55 · value `1` |
| 63 | 1 | `repeated_scope_marker` | `u8` | little | spec | scope-record index at offset 64 · value `1` |
| 64 | 4 | `repeated_scope_record_index` | `u32` | little | spec | scope-record index at offset 64 |
| 76 | 1 | `companion_marker` | `u8` | little | spec | companion-record index plus one at offset 77 · value `1` |
| 77 | 4 | `companion_record_index` | `u32` | little | spec | companion-record index plus one at offset 77 |
| 88 | 1 | `final_scope_marker` | `u8` | little | spec | scope-record index at offset 89 · value `1` |
| 89 | 4 | `final_scope_record_index` | `u32` | little | spec | scope-record index at offset 89 |

Unstated regions:

- `11..19` (8 B): The class-426 count-owner prefix reserves eight zero bytes before the first marker.
- `29..35` (6 B): The class-426 count-owner prefix reserves six zero bytes before the local ordinal.
- `49..55` (6 B): The parameter reference closes before the owned ordinal.
- `59..63` (4 B): The class-426 count-owner frame reserves four zero bytes before the repeated scope reference.
- `68..76` (8 B): The repeated scope reference closes before the companion reference.
- `81..88` (7 B): The companion reference closes before the final scope reference.
- `93..99` (6 B): The final scope reference closes immediately before the paired class-267 header.

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

## `design_hole_extent_controls`

Spec §3.1 · layout: byte offsets · size: 4 B

Offsets are relative to the end of the Hole property block, or to the end of the u32 word that type versions 5 and 7 store after it. Type version 3 stores only the two-byte prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/hole.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `prefix` | `bytes[2]` | little | spec | Type version `4` stores the extent controls `01 01 X 00` directly |
| 2 | 1 | `extent_control` | `u8` | little | spec | The extent control `X` is `0` for Distance and `1` for a non-distance extent |
| 3 | 1 | `suffix` | `u8` | little | spec | Type version `4` stores the extent controls `01 01 X 00` directly |

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

## `scale_legacy_uniform_309`

Spec §3.1 · layout: byte offsets · size: 309 B

Offsets are relative to the primary indexed header. The paired same-index header is outside this frame.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/direct_face.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/parameter_scope.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 75 | `operation_prefix` | `bytes[75]` | little | spec | It uses the same 75-byte fixed operation prefix |
| 75 | 4 | `null_guid_code_unit_count` | `u32` | little | spec | A 36-code-unit LP-UTF16 null GUID starts at offset 75 · value `36` |
| 79 | 72 | `null_guid` | `bytes[72]` | little | derived | The GUID follows its four-byte count and occupies two bytes per code unit. |
| 151 | 3 | `zero_before_reference_table` | `bytes[3]` | little | derived | The three zero bytes follow the GUID at offsets 79 through 150. · value `[0, 0, 0]` |
| 154 | 4 | `reference_count` | `u32` | little | spec | The five-entry ordered reference table starts at offset 154 · value `5` |
| 158 | 55 | `references` | `bytes[55]` | little | derived | Five eleven-byte references follow the four-byte count. |
| 213 | 4 | `history_state_id` | `u32` | little | spec | The current history-state identity is at offset 213 |
| 217 | 4 | `kind_code_unit_count` | `u32` | little | spec | the kind code-unit count is at offset 217 · value `5` |
| 221 | 10 | `kind` | `bytes[10]` | little | spec | `Scale` starts at offset 221 |
| 231 | 4 | `feature_ordinal` | `u32` | little | spec | The feature ordinal starts the 78-byte fixed tail at offset 231 |
| 262 | 4 | `previous_history_state_id` | `u32` | little | spec | The preceding history-state identity is at offset 262 |

Unstated regions:

- `235..262` (27 B): Common fixed-tail fields between the ordinal and preceding history state.
- `266..309` (43 B): Common fixed-tail fields up to the paired header at offset 309.

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

## `paramesh_texture_flags_entry`

Spec §3.1 · layout: byte offsets · size: 44 B

Offsets are relative to one flags-map entry. The GUID count is 36.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `guid_count` | `u32` | little | spec | a 36-byte LP-ASCII resource GUID |
| 4 | 36 | `resource_guid` | `bytes[36]` | little | spec | a 36-byte LP-ASCII resource GUID |
| 40 | 4 | `flags` | `u32` | little | spec | followed by an opaque u32 flags value |

## `paramesh_texture_filename_entry`

Spec §3.1 · layout: byte offsets · size: 51 B

Offsets are relative to one filename-map entry. The GUID count is 36.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `guid_count` | `u32` | little | spec | a 36-byte LP-ASCII resource GUID |
| 4 | 36 | `resource_guid` | `bytes[36]` | little | spec | a 36-byte LP-ASCII resource GUID |
| 40 | 11 | `filename_record_reference` | `bytes[11]` | little | spec | followed by a marked same-segment filename-record reference |

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
| 46 | 49 | `footer_mask` | `bytes[49]` | little | spec | remaining 49 bytes |

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

## `paramesh_scene_node_placed`

Spec §3.1 · layout: byte offsets · size: 261 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The placed form shares that header, constants, and references through offset 58 |
| 59 | 25 | `zero_run_25` | `bytes[25]` | little | spec | 25 zero bytes at offset 59 |
| 84 | 128 | `transform` | `bytes[128]` | little | spec | a row-major 4×4 f64 affine transform at offset 84 |
| 212 | 49 | `footer_mask` | `bytes[49]` | little | spec | the 49-byte bounds payload at offset 212 |

Unstated regions:

- `11..59` (48 B): The header, constants, and two references use the compact form's offsets through byte 58.

## `paramesh_collection_owner_backlink_prefix`

Spec §3.1 · layout: byte offsets · size: 273 B

Offsets are relative to the collection-owner indexed header. The record can continue after this fixed backlink prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The owner has type GUID |
| 262 | 11 | `collection_backlink` | `bytes[11]` | little | spec | In version 23, its marked same-segment reference at offset 262 points back to the collection. |

Unstated regions:

- `11..262` (251 B): The owner payload before the reciprocal collection reference is outside this table.

## `paramesh_collection_owner_v17`

Spec §3.1 · layout: byte offsets · size: 252 B

Offsets are relative to the collection-owner indexed header. The record can continue after this fixed backlink prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The owner has type GUID |
| 241 | 11 | `collection_backlink` | `bytes[11]` | little | spec | Version 17 stores its collection backlink at offset 241 |

Unstated regions:

- `11..241` (230 B): The version-17 owner payload before the reciprocal collection reference is outside this table.

## `assembly_class_406_261_scope_671`

Spec §Assembly operands · layout: byte offsets · size: 671 B

Offsets are relative to the primary indexed header. The paired class-261 header starts at offset 671; the frame carries two standard operand transforms and a two-locator path run.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The class-406/261 `Assemble` scope is a 671-byte standard form. |
| 28 | 11 | `first_operand_reference` | `bytes[11]` | little | spec | Its two operand references are at offsets 28 and 168 |
| 40 | 128 | `first_operand_transform` | `f64[16]` | little | spec | their row-major rigid transforms start at offsets 40 and 180 |
| 168 | 11 | `second_operand_reference` | `bytes[11]` | little | spec | Its two operand references are at offsets 28 and 168 |
| 180 | 128 | `second_operand_transform` | `f64[16]` | little | spec | their row-major rigid transforms start at offsets 40 and 180 |
| 384 | 4 | `locator_count` | `u32` | little | spec | The operand-path count is at offset 384 · value `2` |
| 388 | 11 | `first_locator_reference` | `bytes[11]` | little | spec | marked locator references at offsets 388 and 399 |
| 399 | 11 | `second_locator_reference` | `bytes[11]` | little | spec | marked locator references at offsets 388 and 399 |

Unstated regions:

- `11..28` (17 B): The standard scope prologue before the first operand reference is outside this table.
- `39..40` (1 B): One zero byte separates the first operand reference from its transform.
- `179..180` (1 B): One zero byte separates the second operand reference from its transform.
- `308..384` (76 B): The standard frame tail before the locator run is outside this table.
- `410..671` (261 B): The standard frame tail after the locator run is outside this table; the paired header starts at offset 671.

## `assembly_class_383_258_scope_1011`

Spec §Assembly operands · layout: byte offsets · size: 1011 B

Offsets are relative to the primary indexed header. The paired class-258 header starts at offset 1011.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The class-383/258 `Assemble` scope is a 1011-byte standard form. |
| 28 | 11 | `first_operand_reference` | `bytes[11]` | little | spec | The operand references are at scope offsets 28 and 168 |
| 40 | 128 | `first_operand_transform` | `f64[16]` | little | spec | their row-major rigid transforms start at offsets 40 and 180 |
| 168 | 11 | `second_operand_reference` | `bytes[11]` | little | spec | The operand references are at scope offsets 28 and 168 |
| 180 | 128 | `second_operand_transform` | `f64[16]` | little | spec | their row-major rigid transforms start at offsets 40 and 180 |
| 308 | 4 | `standard_tail_marker` | `bytes[4]` | little | spec | Bytes 308 through 311 are zero |

Unstated regions:

- `11..28` (17 B): The fixed scope prologue before the first operand reference is outside this table.
- `39..40` (1 B): One zero byte separates the first operand reference from its transform.
- `179..180` (1 B): One zero byte separates the second operand reference from its transform.
- `312..1011` (699 B): The remainder of the standard scope frame before the paired header is outside this table.

## `assembly_class_388_266_scope_968`

Spec §Assembly operands · layout: byte offsets · size: 968 B

Offsets are relative to the primary indexed header. The paired class-266 header starts at offset 968. The 35-entry reference table is interleaved with six nested construction records and the final four alignment owners.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The class-388/266 `Assemble` scope is a 968-byte legacy form. |
| 20 | 6 | `scope_flags` | `bytes[6]` | little | spec | The class-388 form stores `01 00 00 00 00 01` at offsets 20 through 25. · value `[1, 0, 0, 0, 0, 1]` |
| 28 | 11 | `first_operand_reference` | `bytes[11]` | little | spec | The two marked operand references are at offsets 28 and 168. |
| 40 | 128 | `first_operand_transform` | `f64[16]` | little | spec | The two row-major rigid operand transforms start at offsets 40 and 180. |
| 168 | 11 | `second_operand_reference` | `bytes[11]` | little | spec | The two marked operand references are at offsets 28 and 168. |
| 180 | 128 | `second_operand_transform` | `f64[16]` | little | spec | The two row-major rigid operand transforms start at offsets 40 and 180. |
| 362 | 4 | `operand_path_locator_count` | `u32` | little | spec | The class-388 form stores operand-path locator count 2 at offset 362. · value `2` |
| 366 | 22 | `operand_path_locator_references` | `bytes[22]` | little | spec | The two marked operand-path locator references occupy offsets 366 and 377. |
| 388 | 11 | `external_component_reference` | `bytes[11]` | little | spec | The marked external component reference follows the operand-path locator run. |
| 399 | 76 | `component_identity` | `bytes[76]` | little | spec | The class-388 form stores a 36-code-unit LP-UTF16 component identity at offset 399. |
| 478 | 4 | `reference_count` | `u32` | little | spec | The class-388 form stores u32 value 35 at offset 478. · value `35` |
| 482 | 385 | `reference_entries` | `bytes[385]` | little | spec | Thirty-five marked same-segment reference entries occupy offsets 482 through 866. |
| 867 | 4 | `reference_trailer` | `bytes[4]` | little | spec | The fixed scope trailer stores four `ff` bytes at offset 867. · value `[255, 255, 255, 255]` |
| 871 | 4 | `kind_code_unit_count` | `u32` | little | spec | The UTF-16LE kind length is 8 at offset 871. · value `8` |
| 875 | 16 | `kind` | `bytes[16]` | little | spec | The kind payload is `Assemble`. |
| 891 | 4 | `feature_ordinal` | `u32` | little | spec | The feature ordinal follows the kind payload at offset 891. |

Unstated regions:

- `11..20` (9 B): Bytes 11 through 19 are zero.
- `26..28` (2 B): Bytes 26 and 27 are zero.
- `39..40` (1 B): One zero byte separates the first operand reference from its transform.
- `179..180` (1 B): One zero byte separates the second operand reference from its transform.
- `308..362` (54 B): The fixed construction tail before the typed external-reference run is outside this table.
- `475..478` (3 B): Three zero bytes precede the ordered scope reference table.
- `895..968` (73 B): The fixed class-388 tail before the paired class-266 header is outside this table.

## `assembly_legacy_class_412_path_425`

Spec §Assembly operands · layout: byte offsets · size: 425 B

Offsets are relative to the class-412 indexed header. Each GUID field is a 36-code-unit LP-UTF16 value.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A class-412 path record is 425 bytes in the class-388 legacy envelope. |
| 21 | 1 | `path_marker` | `u8` | little | spec | The class-412 path prefix stores ten zero bytes, marker 1 at offset 21, and three zero bytes at offsets 22 through 24. · value `1` |
| 25 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | A class-412 path stores one occurrence GUID followed by four identity GUIDs. |
| 101 | 76 | `first_identity_guid` | `bytes[76]` | little | spec | A class-412 path stores one occurrence GUID followed by four identity GUIDs. |
| 177 | 76 | `second_identity_guid` | `bytes[76]` | little | spec | A class-412 path stores one occurrence GUID followed by four identity GUIDs. |
| 253 | 8 | `identity_separator` | `u64` | little | spec | The class-412 path stores u64 value 2 between its identity pairs. · value `2` |
| 261 | 76 | `third_identity_guid` | `bytes[76]` | little | spec | A class-412 path stores one occurrence GUID followed by four identity GUIDs. |
| 337 | 76 | `fourth_identity_guid` | `bytes[76]` | little | spec | A class-412 path stores one occurrence GUID followed by four identity GUIDs. |
| 413 | 4 | `path_tail_count` | `u32` | little | spec | The class-412 path stores u32 value 2 before its zero tail. · value `2` |

Unstated regions:

- `11..21` (10 B): Ten zero bytes follow the indexed header.
- `22..25` (3 B): Three zero bytes follow the class-412 path marker.
- `417..425` (8 B): The class-412 path ends with eight zero bytes.

## `assembly_legacy_class_369_path_wrapper_one`

Spec §Assembly operands · layout: byte offsets · size: 37 B

Offsets are relative to the class-369 indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A class-369 wrapper with path count 1 is 37 bytes. |
| 21 | 1 | `wrapper_marker` | `u8` | little | spec | A class-369 wrapper stores marker 1 and its path count. · value `1` |
| 22 | 4 | `path_count` | `u32` | little | spec | A class-369 wrapper with path count 1 is 37 bytes. · value `1` |
| 26 | 11 | `path_reference` | `bytes[11]` | little | spec | A class-369 wrapper stores one marked path reference at offset 26. |

Unstated regions:

- `11..21` (10 B): Ten zero bytes follow the indexed header.

## `assembly_legacy_class_369_path_wrapper_two`

Spec §Assembly operands · layout: byte offsets · size: 48 B

Offsets are relative to the class-369 indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A class-369 wrapper with path count 2 is 48 bytes. |
| 21 | 1 | `wrapper_marker` | `u8` | little | spec | A class-369 wrapper stores marker 1 and its path count. · value `1` |
| 22 | 4 | `path_count` | `u32` | little | spec | A class-369 wrapper with path count 2 is 48 bytes. · value `2` |
| 26 | 22 | `path_references` | `bytes[22]` | little | spec | A class-369 wrapper stores two marked path references at offsets 26 and 37. |

Unstated regions:

- `11..21` (10 B): Ten zero bytes follow the indexed header.

## `assembly_class_383_258_frame_387_leading`

Spec §Assembly operands · layout: byte offsets · size: 188 B

Offsets are relative to the class-387 primary indexed header; the paired class-258 header starts at offset 188.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The leading and child class-387 frames are 188 and 199 bytes |
| 70 | 11 | `identity_reference` | `bytes[11]` | little | spec | The leading class-387 frame references its class-359 identity at +70 |
| 177 | 11 | `scope_reference` | `bytes[11]` | little | spec | the owning scope at +177 |

Unstated regions:

- `11..70` (59 B): The leading class-387 payload before its identity reference is outside this table.
- `81..177` (96 B): The leading class-387 payload before its scope backlink is outside this table.

## `assembly_class_383_258_frame_387_child`

Spec §Assembly operands · layout: byte offsets · size: 199 B

Offsets are relative to the class-387 primary indexed header; the paired class-258 header starts at offset 199.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The leading and child class-387 frames are 188 and 199 bytes |
| 70 | 11 | `identity_reference` | `bytes[11]` | little | spec | The child class-387 frame references its class-359 identity at +70 |
| 81 | 11 | `leading_reference` | `bytes[11]` | little | spec | its leading class-387 frame at +81 |
| 188 | 11 | `scope_reference` | `bytes[11]` | little | spec | the owning scope at +188 |

Unstated regions:

- `11..70` (59 B): The child class-387 payload before its identity reference is outside this table.
- `92..188` (96 B): The child class-387 payload before its scope backlink is outside this table.

## `assembly_class_383_258_frame_394`

Spec §Assembly operands · layout: byte offsets · size: 215 B

Offsets are relative to the class-394 primary indexed header; the paired class-258 header starts at offset 215.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | each class-394 frame is 215 bytes |
| 97 | 11 | `identity_reference` | `bytes[11]` | little | spec | Each class-394 frame references its class-359 identity at +97 |
| 204 | 11 | `scope_reference` | `bytes[11]` | little | spec | the owning scope at +204 |

Unstated regions:

- `11..97` (86 B): The class-394 payload before its identity reference is outside this table.
- `108..204` (96 B): The class-394 payload before its scope backlink is outside this table.

## `assembly_class_383_258_frame_359_identity`

Spec §Assembly operands · layout: byte offsets · size: 282 B

Offsets are relative to the class-359 primary indexed header; the paired class-258 header starts at offset 282.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | every class-359 frame is 282 bytes |
| 37 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | two 36-code-unit LP-UTF16 GUIDs at +37 and +113 |
| 113 | 76 | `identity_guid` | `bytes[76]` | little | spec | two 36-code-unit LP-UTF16 GUIDs at +37 and +113 |
| 271 | 11 | `scope_reference` | `bytes[11]` | little | spec | the owning scope reference at +271 |

Unstated regions:

- `11..37` (26 B): The class-359 payload before its first GUID is outside this table.
- `189..271` (82 B): The class-359 payload between its GUIDs and scope backlink is outside this table.

## `assembly_class_383_258_frame_378_carrier`

Spec §Assembly operands · layout: byte offsets · size: 410 B

Offsets are relative to the class-378 primary indexed header; the paired class-258 header starts at offset 410.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Each operand carrier is a 410-byte class-378 frame. |
| 49 | 128 | `transform` | `f64[16]` | little | spec | Its matrix at +49 equals the corresponding scope transform |
| 197 | 11 | `child_reference` | `bytes[11]` | little | spec | It references the child class-387 frame at +197 |
| 212 | 11 | `second_face_reference` | `bytes[11]` | little | spec | the second class-394 frame at +212 |
| 223 | 11 | `first_face_reference` | `bytes[11]` | little | spec | the first class-394 frame at +223 |
| 242 | 44 | `placement_owner_references` | `bytes[44]` | little | spec | Four placement-owner references occupy +242, +253, +264, and +275 |
| 290 | 11 | `repeated_child_reference` | `bytes[11]` | little | spec | Repeated references to the child, first face, and second face occur at +290 |
| 301 | 11 | `repeated_first_face_reference` | `bytes[11]` | little | spec | first face, and second face occur at +290, +301, and +312 |
| 312 | 11 | `repeated_second_face_reference` | `bytes[11]` | little | spec | second face occur at +290, +301, and +312 |
| 399 | 11 | `scope_reference` | `bytes[11]` | little | spec | the owning scope at +399 |

Unstated regions:

- `11..49` (38 B): The class-378 carrier payload before its matrix is outside this table.
- `177..197` (20 B): The class-378 carrier payload before its first child reference is outside this table.
- `208..212` (4 B): The class-378 carrier payload before its second face reference is outside this table.
- `234..242` (8 B): The class-378 carrier payload before its placement-owner reference run is outside this table.
- `286..290` (4 B): The class-378 carrier payload before its repeated child reference is outside this table.
- `323..399` (76 B): The class-378 carrier payload before its scope backlink is outside this table.

## `assembly_class_363_264_frame_360_leading`

Spec §Assembly operands · layout: byte offsets · size: 188 B

Offsets are relative to the class-360 primary indexed header; the paired class-264 header starts at offset 188.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its 188-byte leading form |
| 70 | 11 | `identity_reference` | `bytes[11]` | little | spec | A class-360 node references its class-388 identity at +70 |
| 177 | 11 | `scope_reference` | `bytes[11]` | little | spec | references the scope at +177 |

Unstated regions:

- `11..70` (59 B): Fixed payload before the identity reference.
- `81..177` (96 B): Fixed payload before the scope backlink.

## `assembly_class_363_264_frame_360_child`

Spec §Assembly operands · layout: byte offsets · size: 199 B

Offsets are relative to the class-360 primary indexed header; the paired class-264 header starts at offset 199.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its 199-byte child form |
| 70 | 11 | `identity_reference` | `bytes[11]` | little | spec | A class-360 node references its class-388 identity at +70 |
| 81 | 11 | `leading_reference` | `bytes[11]` | little | spec | references the leading class-360 node at +81 |
| 188 | 11 | `scope_reference` | `bytes[11]` | little | spec | and the scope at +188 |

Unstated regions:

- `11..70` (59 B): Fixed payload before the identity reference.
- `92..188` (96 B): Fixed payload before the scope backlink.

## `assembly_class_363_264_frame_386_terminal`

Spec §Assembly operands · layout: byte offsets · size: 215 B

Offsets are relative to the class-386 primary indexed header; the paired class-264 header starts at offset 215.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A terminal class-386 node is 215 bytes |
| 97 | 11 | `identity_reference` | `bytes[11]` | little | spec | it references its class-388 identity at +97 |
| 204 | 11 | `scope_reference` | `bytes[11]` | little | spec | and the scope at +204 |

Unstated regions:

- `11..97` (86 B): Fixed payload before the identity reference.
- `108..204` (96 B): Fixed payload before the scope backlink.

## `assembly_class_363_264_frame_388_identity`

Spec §Assembly operands · layout: byte offsets · size: 614 B

Offsets are relative to the class-388 primary indexed header; the paired class-264 header starts at offset 614.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its compact form is 614 bytes |
| 37 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | A class-388 identity stores the occurrence GUID at +37 |
| 113 | 76 | `component_identity_guid` | `bytes[76]` | little | spec | component identity GUID at +113 |
| 603 | 11 | `scope_reference` | `bytes[11]` | little | spec | backlinks at +603 |

Unstated regions:

- `11..37` (26 B): Fixed identity prefix.
- `189..603` (414 B): Typed external-identity payload before the scope backlink.

## `assembly_class_363_264_frame_388_identity_short`

Spec §Assembly operands · layout: byte offsets · size: 612 B

Offsets are relative to the class-388 primary indexed header; the paired class-264 header starts at offset 612.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its short compact form is 612 bytes |
| 37 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | A class-388 identity stores the occurrence GUID at +37 |
| 113 | 76 | `component_identity_guid` | `bytes[76]` | little | spec | component identity GUID at +113 |
| 601 | 11 | `scope_reference` | `bytes[11]` | little | spec | backlinks to the scope at +601 |

Unstated regions:

- `11..37` (26 B): Fixed identity prefix.
- `189..601` (412 B): Short typed external-identity payload before the scope backlink.

## `assembly_class_363_264_frame_388_identity_extended`

Spec §Assembly operands · layout: byte offsets · size: 690 B

Offsets are relative to the class-388 primary indexed header; the paired class-264 header starts at offset 690.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its extended form is 690 bytes |
| 37 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | A class-388 identity stores the occurrence GUID at +37 |
| 113 | 76 | `component_identity_guid` | `bytes[76]` | little | spec | component identity GUID at +113 |
| 679 | 11 | `scope_reference` | `bytes[11]` | little | spec | backlinks at +679 |

Unstated regions:

- `11..37` (26 B): Fixed identity prefix.
- `189..679` (490 B): Extended typed external-identity payload before the scope backlink.

## `assembly_class_363_264_frame_388_identity_reduced_490`

Spec §Assembly operands · layout: byte offsets · size: 490 B

Offsets are relative to the class-388 primary indexed header; the paired class-264 header starts at offset 490.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Two reduced forms are 490 and 501 bytes |
| 37 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | A class-388 identity stores the occurrence GUID at +37 |
| 113 | 76 | `component_identity_guid` | `bytes[76]` | little | spec | component identity GUID at +113 |
| 479 | 11 | `scope_reference` | `bytes[11]` | little | spec | their scope backlinks are at +479 and +490 |

Unstated regions:

- `11..37` (26 B): Fixed identity prefix.
- `189..479` (290 B): Reduced typed identity payload before the scope backlink.

## `assembly_class_363_264_frame_388_identity_reduced_501`

Spec §Assembly operands · layout: byte offsets · size: 501 B

Offsets are relative to the class-388 primary indexed header; the paired class-264 header starts at offset 501.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Two reduced forms are 490 and 501 bytes |
| 37 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | A class-388 identity stores the occurrence GUID at +37 |
| 113 | 76 | `component_identity_guid` | `bytes[76]` | little | spec | component identity GUID at +113 |
| 490 | 11 | `scope_reference` | `bytes[11]` | little | spec | their scope backlinks are at +479 and +490 |

Unstated regions:

- `11..37` (26 B): Fixed identity prefix.
- `189..490` (301 B): Reduced typed identity payload before the scope backlink.

## `assembly_class_307_264_joint_origin_scope`

Spec §Assembly operands · layout: byte offsets · size: 366 B

Offsets are relative to the class-307 primary indexed header; the paired class-264 header starts at offset 366.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_carrier_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The class-307 JointOrigin scope is 366 bytes |
| 21 | 11 | `first_reference` | `bytes[11]` | little | spec | Marked references occur at +21 and +33 |
| 33 | 11 | `second_reference` | `bytes[11]` | little | spec | Marked references occur at +21 and +33 |
| 44 | 76 | `identity_guid` | `bytes[76]` | little | spec | One 36-code-unit identity GUID starts at +44 |
| 120 | 3 | `identity_separator` | `bytes[3]` | little | spec | A three-byte separator starts at +120 |
| 123 | 4 | `reference_count` | `u32` | little | spec | A u32 reference count 12 at +123 · value `12` |
| 127 | 132 | `reference_entries` | `bytes[132]` | little | spec | twelve marked references starting at +127 |
| 259 | 4 | `reference_trailer` | `bytes[4]` | little | spec | Four FF bytes follow the reference table · value `[255, 255, 255, 255]` |
| 263 | 4 | `kind_code_unit_count` | `u32` | little | spec | The 11-code-unit `JointOrigin` kind starts at +263 · value `11` |
| 267 | 22 | `kind` | `bytes[22]` | little | spec | The 11-code-unit `JointOrigin` kind starts at +263 |
| 289 | 4 | `feature_ordinal` | `u32` | little | spec | the feature ordinal is at +289 |

Unstated regions:

- `11..21` (10 B): Fixed scope prefix.
- `32..33` (1 B): Fixed separator.
- `293..366` (73 B): Fixed scope tail.

## `assembly_class_363_264_frame_363_carrier`

Spec §Assembly operands · layout: byte offsets · size: 389 B

Offsets are relative to the class-363 primary indexed header; the paired class-264 header starts at offset 389.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The class-363 frame is 389 bytes |
| 49 | 128 | `transform` | `f64[16]` | little | spec | Its rigid transform starts at +49 |
| 197 | 11 | `leading_reference` | `bytes[11]` | little | spec | References at +197 and +213 name a leading class-360 node |
| 213 | 11 | `terminal_reference` | `bytes[11]` | little | spec | and a terminal class-386 node |
| 232 | 44 | `placement_owner_references` | `bytes[44]` | little | spec | Four placement-owner references occupy +232 through +275 |
| 280 | 11 | `repeated_leading_reference` | `bytes[11]` | little | spec | The leading and terminal references repeat at +280 and +291 |
| 291 | 11 | `repeated_terminal_reference` | `bytes[11]` | little | spec | The leading and terminal references repeat at +280 and +291 |
| 378 | 11 | `scope_reference` | `bytes[11]` | little | spec | The owning-scope backlink is at +378 |

Unstated regions:

- `11..49` (38 B): Fixed carrier prefix.
- `177..197` (20 B): Fixed payload before the node references.
- `208..213` (5 B): Fixed separator before the terminal reference.
- `224..232` (8 B): Fixed counts before the placement-owner references.
- `276..280` (4 B): Fixed count before repeated references.
- `302..378` (76 B): Fixed carrier payload before the scope backlink.

## `assembly_variable_reference_operand_path_locator`

Spec §Assembly operands · layout: byte offsets · size: 180 B

Offsets are relative to the locator's indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 23 | 128 | `transform` | `f64[16]` | little | spec | Its rigid transform starts at offset 23 |
| 151 | 1 | `zero_separator` | `u8` | little | spec | one zero separator is at offset 151 |
| 152 | 11 | `scope_backlink` | `bytes[11]` | little | spec | its owning-scope backlink is at offset 152 |
| 163 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | its path-wrapper reference is at offset 163 |
| 174 | 4 | `constant_two` | `u32` | little | spec | u32 value 2 is at offset 174 |
| 178 | 2 | `zero_tail` | `bytes[2]` | little | spec | two zero tail bytes occupy offsets 178 and 179 |

Unstated regions:

- `0..23` (23 B): Indexed header and fixed zero prefix.

## `assembly_variable_reference_path_wrapper_continuation`

Spec §Assembly operands · layout: byte offsets · size: 11 B

One entry is repeated for each path record after the first.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `path_reference` | `bytes[11]` | little | spec | one marked reference |

## `assembly_operand_path_locator_reference_run`

Spec §Assembly operands · layout: byte offsets · size: 26 B

Offsets are relative to the count. The run starts at scope offset 47 in the 378- and 399-byte As-built forms, offset 69 in the 422-byte As-built form, offset 80 in the 444-byte As-built form, offset 362 in the 627-, 637-, 692-, and 748-byte forms and the variable-span class-283/264 and class-347/260 forms, offset 358 in the 633-, 732-, and 744-byte forms, offset 537 in the 945-byte form, offset 530 in the 806- and 861-byte forms, and offset 552 in the 850-byte form.

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
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The class-430/262 748-byte Assemble form uses the standard locator-reference offsets.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The variable-span class-283/264 and class-347/260 Assemble forms use the standard locator-reference offsets and are admitted by class pair.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The 633-byte compact assembly form uses the two tabulated scope-relative locator-reference offsets.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The 732-byte compact assembly form uses the two tabulated scope-relative locator-reference offsets.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The class-430/262 744-byte Assemble form uses the compact locator-reference offsets.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The 945-byte wide Assemble form stores its locator references after the count at offset 537.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The 806- and 861-byte compact wide Assemble forms store their locator references after the count at offset 530.
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs` — The 850-byte compact wide Assemble form stores its locator references after the count at offset 552, behind its two limit-owner references.

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

Offsets are relative to the recipe prefix. N variable-length counted operand groups and one final u32 zero follow this fixed header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 10 | `zero_run_10` | `bytes[10]` | little | spec | stores ten zero bytes |
| 10 | 4 | `constant_one` | `u32` | little | spec | `u32 1` |
| 14 | 4 | `group_count` | `u32` | little | spec | a `u32 N` with `N >= 4`, and exactly `N` groups |

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
| 6 | 8 | `selection_payload_stamp` | `u64` | little | spec | a u64 selection-payload stamp |
| 14 | 4 | `evaluation_epoch` | `u32` | little | spec | a u32 evaluation epoch |
| 18 | 8 | `evaluation_scalar` | `f64` | little | spec | a finite nonnegative f64 evaluation scalar |
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

## `typed_reference_box_primitive_1295_frame`

Spec §3.1 · layout: byte offsets · size: 1295 B

Offsets are relative to the primary indexed scope header. The generic scope reference table begins at offset 655; the paired header follows the complete scope frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its bytes 11 through 20 are zero, byte 21 is `0x01`, the result-operation u32 is at offset 22 with the four values of the named form |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Its bytes 11 through 20 are zero, byte 21 is `0x01`, the result-operation u32 is at offset 22 with the four values of the named form |
| 21 | 1 | `form_marker` | `u8` | little | spec | byte 21 is `0x01`, the result-operation u32 is at offset 22 with the four values of the named form · value `1` |
| 22 | 4 | `operation` | `u32` | little | spec | byte 21 is `0x01`, the result-operation u32 is at offset 22 with the four values of the named form |
| 26 | 1 | `nested_marker` | `u8` | little | spec | byte 26 is `0x01` · value `1` |
| 27 | 357 | `typed_references` | `bytes[357]` | little | spec | Seven typed local references begin at offsets 27, 78, 129, 180, 231, 282, and 333 |
| 384 | 1 | `zero_before_matrix` | `u8` | little | spec | Byte 384 is zero |
| 385 | 128 | `matrix` | `f64[16]` | little | spec | A row-major rigid right-handed 4x4 f64 frame starts at offset 385 |
| 513 | 4 | `one_count` | `u32` | little | spec | The u32 `1` at offset 513 precedes one typed local reference at offset 517 · value `1` |
| 517 | 51 | `tail_reference` | `bytes[51]` | little | spec | one typed local reference at offset 517 |
| 568 | 8 | `zero_run_8` | `bytes[8]` | little | spec | Eight zero bytes at offsets 568 through 575 |
| 576 | 4 | `guid_code_unit_count` | `u32` | little | spec | 36-code-unit LP-UTF16 null GUID at offset 576 · value `36` |
| 580 | 72 | `guid` | `bytes[72]` | little | spec | 36-code-unit LP-UTF16 null GUID at offset 576 |
| 652 | 3 | `zero_run_3_after_guid` | `bytes[3]` | little | spec | Bytes 652 through 654 are zero |
| 655 | 4 | `reference_count` | `u32` | little | spec | the reference count at offset 655 · value `8` |
| 659 | 408 | `references` | `bytes[408]` | little | spec | the ordered 51-byte typed reference run at offset 659 |
| 1067 | 4 | `history_state_id` | `u32` | little | spec | the current history state at offset 1067 |
| 1071 | 4 | `kind_code_unit_count` | `u32` | little | spec | the 12-code-unit `BoxPrimitive` kind at offset 1071 · value `12` |
| 1075 | 24 | `kind` | `bytes[24]` | little | spec | the 12-code-unit `BoxPrimitive` kind at offset 1071 |
| 1099 | 4 | `feature_ordinal` | `u32` | little | spec | the feature ordinal at offset 1099 |
| 1169 | 4 | `previous_history_state_id` | `u32` | little | spec | the previous history state at offset 1169 |

Unstated regions:

- `1103..1169` (66 B): Scope-tail bytes between the feature ordinal and previous history state.
- `1173..1295` (122 B): Scope-tail bytes before the paired indexed header.

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

## `legacy_loft_body_carrier_class_322`

Spec §3.1 · layout: byte offsets · size: 87 B

Offsets are relative to the primary indexed header. The same-index paired header begins at offset 87.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | starts with the indexed header at offsets 0 through 10 |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | has ten zero bytes at offsets 11 through 20 |
| 21 | 1 | `presence` | `u8` | little | spec | uses offset 21 as a `01` presence byte |
| 22 | 4 | `owner_scope_record_index` | `u32` | little | spec | The owning scope record is the u32 at offset 22 |
| 26 | 6 | `zero_run_6` | `bytes[6]` | little | spec | offsets 26 through 31 are zero |
| 32 | 4 | `member_count` | `u32` | little | spec | u32 `member_count` is at offset 32 |
| 36 | 11 | `member_reference` | `bytes[11]` | little | spec | its one same-segment member reference starts at offset 36 |
| 47 | 4 | `opaque_index` | `u32` | little | spec | The opaque u32 index is at offset 47 |
| 51 | 8 | `opaque_scalar` | `f64` | little | spec | the finite f64 scalar is at offset 51 |
| 59 | 4 | `repeated_opaque_index` | `u32` | little | spec | the repeated u32 index is at offset 59 |
| 63 | 11 | `next_next_reference` | `bytes[11]` | little | spec | A marked reference to record `N+2` starts at offset 63 |
| 74 | 2 | `flags` | `bytes[2]` | little | spec | flags `[00 00]` are at offset 74 |
| 76 | 11 | `next_reference` | `bytes[11]` | little | spec | a marked reference to record `N+1` starts at offset 76 |

## `legacy_loft_body_carrier_class_322_tail`

Spec §3.1 · layout: byte offsets · size: 99 B

Offsets are relative to the primary indexed header. The same-index paired header begins at offset 99.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 87 | `common_prefix` | `bytes[87]` | little | spec | uses the class-`322`/`262` carrier prefix through offset 86 |
| 87 | 1 | `tail_zero` | `u8` | little | spec | stores a zero byte at offset 87 |
| 88 | 11 | `trailing_scope_reference` | `bytes[11]` | little | spec | a marked reference to the owning scope at offset 88 |

## `legacy_loft_body_carrier_class_411`

Spec §3.1 · layout: byte offsets · size: 99 B

Offsets are relative to the primary indexed header. The class-specific owner tail occupies offsets 87 through 98.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 87 | `common_prefix` | `bytes[87]` | little | spec | The class-`322`/`262` legacy Boolean-Loft body carrier is 87 bytes |
| 87 | 1 | `tail_zero` | `u8` | little | spec | stores a zero byte at offset 87 |
| 88 | 11 | `trailing_scope_reference` | `bytes[11]` | little | spec | a marked reference to the owning scope at offset 88 |

## `fixed_pipe_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 28 B

Offsets are relative to the primary indexed header. The variable reference table follows the fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | eleven-byte indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes at offsets 11 through 20 |
| 21 | 1 | `is_hollow` | `u8` | little | spec | the `isHollow` Boolean at offset 21 |
| 22 | 4 | `operation` | `u32` | little | spec | the result-operation u32 at offset 22 |
| 26 | 2 | `opaque_post_operation` | `bytes[2]` | little | spec | two opaque bytes at offsets 26 and 27 |

## `legacy_pipe_operation_prefix`

Spec §3.1 · layout: byte offsets · size: 32 B

Offsets are relative to the primary indexed header. The variable reference table follows the fixed prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | eleven-byte indexed header |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | nine zero bytes at offsets 11 through 19 |
| 20 | 1 | `prefix_marker` | `u8` | little | spec | u8 `1` at offset 20 · value `1` |
| 21 | 4 | `zero_run_4` | `bytes[4]` | little | spec | four zero bytes at offsets 21 through 24 |
| 25 | 1 | `is_hollow` | `u8` | little | spec | the `isHollow` Boolean at offset 25 |
| 26 | 4 | `operation` | `u32` | little | spec | the result-operation u32 at offset 26 |
| 30 | 2 | `opaque_post_operation` | `bytes[2]` | little | spec | two opaque bytes at offsets 30 and 31 |

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

Offsets are relative to the primary indexed header for either admitted class pair; the generic scope suffix and paired header remain part of the fixed frame.

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

## `base_feature_class_452_262_compact`

Spec §3.1 · layout: byte offsets · size: 348 B

Offsets are relative to the primary indexed header. This exact form has one output body and 64-bit references in its legacy lanes.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | bytes 11 through 18 are zero |
| 19 | 1 | `body_count_marker` | `u8` | little | spec | byte 19 is `0x01` · value `1` |
| 20 | 4 | `body_count` | `u32` | little | spec | offset 20 stores u32 `1` · value `1` |
| 24 | 1 | `body_entity_reference_marker` | `u8` | little | spec | marked 15-byte body entity element at offset 24 · value `1` |
| 25 | 8 | `body_entity_suffix` | `u64` | little | spec | one u64 Design body entity suffix |
| 33 | 6 | `body_entity_reference_field` | `bytes[6]` | little | spec | six-byte source field |
| 39 | 1 | `tag_body_based_on_faces_marker` | `u8` | little | spec | property block at offset 39 · value `1` |
| 40 | 4 | `tag_body_based_on_faces_count` | `u32` | little | spec | one `TagBodyBasedOnFaces` entry · value `1` |
| 44 | 4 | `tag_body_based_on_faces_key_length` | `u32` | little | spec | TagBodyBasedOnFaces · value `19` |
| 48 | 19 | `tag_body_based_on_faces_key` | `bytes[19]` | little | spec | TagBodyBasedOnFaces |
| 67 | 4 | `tag_body_based_on_faces_type_length` | `u32` | little | spec | type `IntrinsicMetaTypebool` · value `21` |
| 71 | 21 | `tag_body_based_on_faces_type` | `bytes[21]` | little | spec | type `IntrinsicMetaTypebool` |
| 92 | 2 | `tag_body_based_on_faces_value` | `u16` | little | spec | u16 value `1` · value `1` |
| 94 | 1 | `mode` | `u8` | little | spec | mode byte at offset 94 |
| 95 | 1 | `parameter_body_count` | `u8` | little | spec | u8 count `1` at offset 95 · value `1` |
| 96 | 3 | `parameter_body_zero_run` | `bytes[3]` | little | spec | three zero bytes |
| 99 | 1 | `parameter_body_reference_marker` | `u8` | little | spec | marked u64 PM body-reference record at offset 99 · value `1` |
| 100 | 8 | `parameter_body_record` | `u64` | little | spec | marked u64 PM body-reference record at offset 99 |
| 108 | 3 | `parameter_body_reference_field` | `bytes[3]` | little | spec | three zero bytes |
| 111 | 1 | `scope_reference_marker` | `u8` | little | spec | marked u64 copy of the scope reference starts at offset 111 · value `1` |
| 112 | 8 | `scope_reference` | `u64` | little | spec | marked u64 copy of the scope reference starts at offset 111 |
| 120 | 2 | `scope_reference_field` | `bytes[2]` | little | spec | two zero bytes |
| 122 | 1 | `auxiliary_group_marker` | `u8` | little | spec | auxiliary group starts at offset 122 · value `1` |
| 123 | 3 | `auxiliary_group_zero_run` | `bytes[3]` | little | spec | three zero bytes |
| 126 | 1 | `auxiliary_reference_marker` | `u8` | little | spec | marked u64 DM body-reference record at offset 126 · value `1` |
| 127 | 8 | `auxiliary_record` | `u64` | little | spec | marked u64 DM body-reference record at offset 126 |
| 135 | 10 | `auxiliary_reference_field` | `bytes[10]` | little | spec | ten zero bytes |
| 145 | 4 | `envelope_guid_code_unit_count` | `u32` | little | spec | 36-code-unit LP-UTF16 envelope GUID starts at offset 145 · value `36` |
| 149 | 72 | `envelope_guid` | `bytes[72]` | little | spec | envelope GUID starts at offset 145 |
| 221 | 3 | `zero_run_after_guid` | `bytes[3]` | little | spec | three zero bytes at offsets 221 through 223 |
| 224 | 4 | `reference_count` | `u32` | little | spec | reference count is at offset 224 · value `1` |
| 228 | 1 | `generic_scope_reference_marker` | `u8` | little | spec | marked u32 scope reference at offset 228 · value `1` |
| 229 | 4 | `generic_scope_reference_record` | `u32` | little | spec | marked u32 scope reference |
| 233 | 6 | `generic_scope_reference_field` | `bytes[6]` | little | spec | six zero bytes |
| 239 | 4 | `history_state_id` | `u32` | little | spec | current history-state identity at offset 239 |
| 243 | 4 | `kind_length` | `u32` | little | spec | kind at offset 247 · value `12` |
| 247 | 24 | `kind` | `bytes[24]` | little | spec | kind at offset 247 |
| 271 | 4 | `feature_ordinal` | `u32` | little | spec | feature ordinal at offset 271 |
| 302 | 4 | `previous_history_state_id` | `u32` | little | spec | previous history-state identity at offset 302 |

Unstated regions:

- `0..11` (11 B): The indexed header precedes the class-452 envelope payload.
- `275..302` (27 B): The post-ordinal scope tail precedes the previous history-state identity.
- `306..348` (42 B): The fixed compact tail closes the primary frame before its paired header.

## `base_feature_class_452_262_expanded`

Spec §3.1 · layout: byte offsets · size: 395 B

Offsets are relative to the primary indexed header. This exact form has two output bodies and counted 32-bit PM and DM reference runs.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | bytes 11 through 18 are zero |
| 19 | 1 | `body_count_marker` | `u8` | little | spec | byte 19 is `0x01` · value `1` |
| 20 | 4 | `body_count` | `u32` | little | spec | offset 20 stores u32 `2` · value `2` |
| 24 | 1 | `body_entity_one_marker` | `u8` | little | spec | Marked 15-byte body entity elements at offsets 24 and 39 · value `1` |
| 25 | 8 | `body_entity_one_suffix` | `u64` | little | spec | two ordered u64 body entity suffixes |
| 33 | 6 | `body_entity_one_field` | `bytes[6]` | little | spec | six-byte source fields |
| 39 | 1 | `body_entity_two_marker` | `u8` | little | spec | Marked 15-byte body entity elements at offsets 24 and 39 · value `1` |
| 40 | 8 | `body_entity_two_suffix` | `u64` | little | spec | two ordered u64 body entity suffixes |
| 48 | 6 | `body_entity_two_field` | `bytes[6]` | little | spec | six-byte source fields |
| 54 | 1 | `tag_body_based_on_faces_marker` | `u8` | little | spec | property block starts at offset 54 · value `1` |
| 55 | 4 | `tag_body_based_on_faces_count` | `u32` | little | spec | one `TagBodyBasedOnFaces` entry · value `1` |
| 59 | 4 | `tag_body_based_on_faces_key_length` | `u32` | little | spec | TagBodyBasedOnFaces · value `19` |
| 63 | 19 | `tag_body_based_on_faces_key` | `bytes[19]` | little | spec | TagBodyBasedOnFaces |
| 82 | 4 | `tag_body_based_on_faces_type_length` | `u32` | little | spec | type `IntrinsicMetaTypebool` · value `21` |
| 86 | 21 | `tag_body_based_on_faces_type` | `bytes[21]` | little | spec | type `IntrinsicMetaTypebool` |
| 107 | 2 | `tag_body_based_on_faces_value` | `u16` | little | spec | u16 value `1` · value `1` |
| 109 | 1 | `parameter_body_group_marker` | `u8` | little | spec | counted PM group starts at offset 109 · value `1` |
| 110 | 4 | `parameter_body_count` | `u32` | little | spec | u32 count `2` · value `2` |
| 114 | 1 | `parameter_body_one_marker` | `u8` | little | spec | eleven-byte members start at offsets 114 and 125 · value `1` |
| 115 | 4 | `parameter_body_one_record` | `u32` | little | spec | eleven-byte members start at offsets 114 and 125 |
| 119 | 6 | `parameter_body_one_field` | `bytes[6]` | little | spec | eleven-byte members start at offsets 114 and 125 |
| 125 | 1 | `parameter_body_two_marker` | `u8` | little | spec | eleven-byte members start at offsets 114 and 125 · value `1` |
| 126 | 4 | `parameter_body_two_record` | `u32` | little | spec | eleven-byte members start at offsets 114 and 125 |
| 130 | 6 | `parameter_body_two_field` | `bytes[6]` | little | spec | eleven-byte members start at offsets 114 and 125 |
| 136 | 1 | `parameter_body_separator` | `u8` | little | spec | zero separator is at offset 136 · value `0` |
| 137 | 1 | `scope_reference_marker` | `u8` | little | spec | marked explicit scope-reference member starts at offset 137 · value `1` |
| 138 | 4 | `scope_reference` | `u32` | little | spec | marked explicit scope-reference member starts at offset 137 |
| 142 | 6 | `scope_reference_field` | `bytes[6]` | little | spec | six zero bytes |
| 148 | 4 | `auxiliary_body_count` | `u32` | little | spec | counted DM group has u32 count `2` at offset 148 · value `2` |
| 152 | 1 | `auxiliary_body_one_marker` | `u8` | little | spec | eleven-byte members start at offsets 152 and 163 · value `1` |
| 153 | 4 | `auxiliary_body_one_record` | `u32` | little | spec | eleven-byte members start at offsets 152 and 163 |
| 157 | 6 | `auxiliary_body_one_field` | `bytes[6]` | little | spec | eleven-byte members start at offsets 152 and 163 |
| 163 | 1 | `auxiliary_body_two_marker` | `u8` | little | spec | eleven-byte members start at offsets 152 and 163 · value `1` |
| 164 | 4 | `auxiliary_body_two_record` | `u32` | little | spec | eleven-byte members start at offsets 152 and 163 |
| 168 | 6 | `auxiliary_body_two_field` | `bytes[6]` | little | spec | eleven-byte members start at offsets 152 and 163 |
| 174 | 8 | `auxiliary_body_zero_run` | `bytes[8]` | little | spec | Eight zero bytes at offsets 174 through 181 |
| 182 | 4 | `envelope_guid_code_unit_count` | `u32` | little | spec | 36-code-unit LP-UTF16 envelope GUID at offset 186 · value `36` |
| 186 | 72 | `envelope_guid` | `bytes[72]` | little | spec | envelope GUID at offset 186 |
| 258 | 3 | `zero_run_after_guid` | `bytes[3]` | little | spec | Three zero bytes at offsets 258 through 260 |
| 261 | 4 | `reference_count` | `u32` | little | spec | reference count is at offset 261 · value `1` |
| 265 | 1 | `generic_scope_reference_marker` | `u8` | little | spec | marked scope reference at offset 265 · value `1` |
| 266 | 4 | `generic_scope_reference_record` | `u32` | little | spec | marked scope reference |
| 270 | 6 | `generic_scope_reference_field` | `bytes[6]` | little | spec | six zero bytes |
| 276 | 4 | `history_state_id` | `u32` | little | spec | current history-state identity at offset 276 |
| 280 | 4 | `kind_length` | `u32` | little | spec | kind at offset 284 · value `12` |
| 284 | 24 | `kind` | `bytes[24]` | little | spec | kind at offset 284 |
| 308 | 4 | `feature_ordinal` | `u32` | little | spec | feature ordinal at offset 308 |
| 349 | 4 | `previous_history_state_id` | `u32` | little | spec | previous history-state identity at offset 349 |

Unstated regions:

- `0..11` (11 B): The indexed header precedes the class-452 envelope payload.
- `312..349` (37 B): The post-ordinal scope tail precedes the previous history-state identity.
- `353..395` (42 B): The fixed expanded tail closes the primary frame before its paired header.

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

## `form_result_owner`

Spec §3.1 · layout: byte offsets · size: 81 B

Offsets are relative to the primary indexed header. Dynamic class tags come from the MetaStream type registry.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | indexed header |
| 11 | 14 | `zero_run_14` | `bytes[14]` | little | spec | fourteen zero bytes |
| 25 | 1 | `owner_marker` | `u8` | little | spec | a marked owning-scope reference |
| 26 | 8 | `owner_scope_record_index` | `u64` | little | spec | a marked owning-scope reference |
| 34 | 24 | `zero_run_24` | `bytes[24]` | little | spec | twenty-four zero bytes |
| 58 | 1 | `body_presentation_marker` | `u8` | little | spec | a marked BodyPresentation reference |
| 59 | 8 | `body_presentation_record_index` | `u64` | little | spec | a marked BodyPresentation reference |
| 67 | 3 | `body_presentation_zero_run` | `bytes[3]` | little | spec | three zero bytes |
| 70 | 1 | `owner_repeat_marker` | `u8` | little | spec | repeated marked owning-scope reference |
| 71 | 8 | `owner_repeat_scope` | `u64` | little | spec | repeated marked owning-scope reference |
| 79 | 2 | `tail_zero_run` | `bytes[2]` | little | spec | two zero bytes |

## `compact_selection_prefix`

Spec §3.1 · layout: byte offsets · size: 36 B

Offsets are relative to the primary indexed header. Ordered 11-byte member references follow this prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | starts with an indexed header |
| 11 | 10 | `zero_run` | `bytes[10]` | little | spec | ten zero bytes |
| 21 | 11 | `owner_reference` | `bytes[11]` | little | spec | a marked owning-scope reference |
| 32 | 4 | `member_count` | `u32` | little | spec | `u32 member_count` |

## `compact_selection_tail_flags_2`

Spec §3.1 · layout: byte offsets · size: 52 B

Offsets are relative to T = 36 + 11 * member_count; the paired header follows this tail.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `ordinal` | `u32` | little | spec | `u32 ordinal` |
| 4 | 8 | `scalar` | `f64` | little | spec | a finite nonnegative `f64 scalar` |
| 12 | 4 | `ordinal_repeat` | `u32` | little | spec | the same `u32 ordinal` |
| 16 | 11 | `first_reference` | `bytes[11]` | little | spec | a marked independent record reference |
| 27 | 2 | `flags` | `bytes[2]` | little | spec | a two- or three-byte flag block |
| 29 | 11 | `next_reference` | `bytes[11]` | little | spec | a marked reference to record `N+1` |
| 40 | 1 | `zero` | `u8` | little | spec | a zero byte |
| 41 | 11 | `owner_reference` | `bytes[11]` | little | spec | a repeated marked owning-scope reference |

## `compact_selection_tail_flags_3`

Spec §3.1 · layout: byte offsets · size: 53 B

Offsets are relative to T = 36 + 11 * member_count; the paired header follows this tail.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `ordinal` | `u32` | little | spec | `u32 ordinal` |
| 4 | 8 | `scalar` | `f64` | little | spec | a finite nonnegative `f64 scalar` |
| 12 | 4 | `ordinal_repeat` | `u32` | little | spec | the same `u32 ordinal` |
| 16 | 11 | `first_reference` | `bytes[11]` | little | spec | a marked independent record reference |
| 27 | 3 | `flags` | `bytes[3]` | little | spec | a two- or three-byte flag block |
| 30 | 11 | `next_reference` | `bytes[11]` | little | spec | a marked reference to record `N+1` |
| 41 | 1 | `zero` | `u8` | little | spec | a zero byte |
| 42 | 11 | `owner_reference` | `bytes[11]` | little | spec | a repeated marked owning-scope reference |

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

Offsets are relative to the serializer's primary indexed header. Classes 315, 349, 360, 431, and 446 use the ordinary paired-header form. Class 335 uses the same 132-byte primary frame followed by a class-331 header with a different record index; its fixed tail after the marked surface reference is zero.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes after its indexed header |
| 21 | 4 | `entry_name_length` | `u32` | little | spec | LP-UTF16 blob-part entry name |

Unstated regions:

- `25..132` (107 B): The LP-UTF16 entry name, marked surface reference, and two-byte zero tail occupy this variable-length region; the complete serializer frame remains 132 bytes.

## `form_class_328_scope`

Spec §1.1.1 · layout: byte offsets · size: 940 B

The scope reference table has exactly two ordered members: one class-417/267 cage group and one class-341/267 cage metadata group. The feature-scope grammar supplies the table offsets and the feature kind.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | the class-328 primary indexed header |

Unstated regions:

- `11..940` (929 B): The class-328 scope payload and fixed history/kind tail are parsed by the Design feature-scope grammar.

## `form_class_328_cage_group`

Spec §1.1.1 · layout: byte offsets · size: 136 B

Offsets are relative to the class-417 primary indexed header. The four class-350/351 member records are followed by three typed tail references: class 272, class 404, and the owning class-328 scope.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | the class-417 primary indexed header |
| 11 | 14 | `zero_run_14` | `bytes[14]` | little | spec | fourteen zero bytes |
| 25 | 1 | `owner_marker` | `u8` | little | spec | marked owning class-328 scope reference · value `1` |
| 26 | 8 | `owner_scope_record_index` | `u64` | little | spec | owning class-328 scope record index |
| 34 | 14 | `zero_run_14_after_owner` | `bytes[14]` | little | spec | fourteen zero bytes before the member count |
| 48 | 4 | `member_count` | `u32` | little | spec | four class-350 member references · value `4` |
| 52 | 44 | `member_entries` | `bytes[44]` | little | spec | four entries of `u8 1 + u64 record index + u16 0`, repeated every 11 bytes |
| 96 | 4 | `terminal_u32` | `u32` | little | spec | fixed u32 value 1 · value `1` |
| 100 | 1 | `first_tail_marker` | `u8` | little | spec | marked class-272 tail reference · value `1` |
| 101 | 8 | `first_tail_record_index` | `u64` | little | spec | class-272 tail record index |
| 109 | 4 | `first_tail_zero_run` | `bytes[4]` | little | spec | four zero bytes |
| 113 | 1 | `second_tail_marker` | `u8` | little | spec | marked class-404 tail reference · value `1` |
| 114 | 8 | `second_tail_record_index` | `u64` | little | spec | class-404 tail record index |
| 122 | 3 | `second_tail_zero_run` | `bytes[3]` | little | spec | three zero bytes |
| 125 | 1 | `final_scope_marker` | `u8` | little | spec | marked repeated class-328 scope reference · value `1` |
| 126 | 8 | `final_scope_record_index` | `u64` | little | spec | repeated owning class-328 scope record index |
| 134 | 2 | `paired_zero_tail` | `bytes[2]` | little | spec | two terminal bytes are zero |

## `form_class_328_reference_entry`

Spec §1.1.1 · layout: byte offsets · size: 11 B

The entry is repeated in the class-417 and class-341 ordered reference runs.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | marked record reference · value `1` |
| 1 | 8 | `record_index` | `u64` | little | spec | referenced record index |
| 9 | 2 | `zero_tail` | `bytes[2]` | little | spec | two zero bytes |

## `form_class_328_metadata_group`

Spec §1.1.1 · layout: byte offsets · size: 293 B

Offsets are relative to the class-341 primary indexed header. The nineteen class-320 member records form the ordered metadata run.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | the class-341 primary indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes |
| 21 | 1 | `owner_marker` | `u8` | little | spec | marked owning class-328 scope reference · value `1` |
| 22 | 8 | `owner_scope_record_index` | `u64` | little | spec | owning class-328 scope record index |
| 30 | 2 | `zero_run_2` | `bytes[2]` | little | spec | two zero bytes |
| 32 | 4 | `member_count` | `u32` | little | spec | nineteen class-320 member references · value `19` |
| 36 | 209 | `member_entries` | `bytes[209]` | little | spec | nineteen entries of `u8 1 + u64 record index + u16 0`, repeated every 11 bytes |
| 245 | 4 | `tail_u32` | `u32` | little | spec | fixed u32 value 61 · value `61` |
| 249 | 8 | `tail_scalar` | `f64` | little | spec | finite little-endian f64 |
| 257 | 1 | `first_tail_marker` | `u8` | little | spec | marked class-259 tail reference · value `1` |
| 258 | 8 | `first_tail_record_index` | `u64` | little | spec | class-259 tail record index |
| 266 | 4 | `first_tail_zero_run` | `bytes[4]` | little | spec | four zero bytes |
| 270 | 1 | `second_tail_marker` | `u8` | little | spec | marked class-404 tail reference · value `1` |
| 271 | 8 | `second_tail_record_index` | `u64` | little | spec | class-404 tail record index |
| 279 | 3 | `second_tail_zero_run` | `bytes[3]` | little | spec | three zero bytes |
| 282 | 1 | `final_scope_marker` | `u8` | little | spec | marked repeated class-328 scope reference · value `1` |
| 283 | 8 | `final_scope_record_index` | `u64` | little | spec | repeated owning class-328 scope record index |
| 291 | 2 | `paired_zero_tail` | `bytes[2]` | little | spec | two terminal bytes are zero |

## `form_class_350_member_owner_tail`

Spec §1.1.1 · layout: byte offsets · size: 13 B

Offsets are relative to the class-351 paired header's preceding tail. The backlink names the class-417 cage group.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `owner_marker` | `u8` | little | spec | marked class-417 group reference · value `1` |
| 1 | 8 | `owner_group_record_index` | `u64` | little | spec | owning class-417 group record index |
| 9 | 3 | `zero_run_3` | `bytes[3]` | little | spec | three zero bytes before the paired header |
| 12 | 1 | `paired_marker` | `u8` | little | spec | a final paired marker · value `1` |

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
| 185 | 1 | `tail_slot_marker` | `u8` | little | spec | a reserved optional-slot marker |
| 186 | 4 | `tail_slot_value` | `u32` | little | spec | followed by u32 zero |

## `coil_compact_scope_discriminators`

Spec §3.1 · layout: byte offsets · size: 111 B

Offsets are relative to the primary indexed scope header. The ordered reference table and scope trailer follow this fixed discriminator block.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 20 | 4 | `operation` | `u32` | little | spec | Offset 20 value `1` creates a new body |
| 24 | 1 | `clockwise` | `u8` | little | spec | `SpirePrimitive` stores the clockwise Boolean at offset 24. |
| 25 | 1 | `left_hand` | `u8` | little | spec | offset 25 is the left-hand Boolean |
| 26 | 4 | `structural_constant` | `u32` | little | spec | offset 26 is u32 `4` |
| 30 | 4 | `extent` | `u32` | little | spec | Offset 30 selects the driving dimensions |
| 92 | 4 | `section_placement` | `u32` | little | spec | Offset 92 selects the section position |
| 107 | 4 | `section_shape` | `u32` | little | spec | Offset 107 selects the section shape |

Unstated regions:

- `11..20` (9 B): The scope framing precedes the fixed Coil discriminators.
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

## `work_plane_legacy_class_322_332_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 332 B

Offsets are relative to the class-322 primary indexed placement header paired with class 261. The zero prefix and matrix match the compact placement family; the remaining tail is opaque.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | The frame stores a row-major 4×4 f64 local-to-model matrix at offset 49 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..49` (38 B): Bytes 11 through 48 are zero.
- `177..332` (155 B): The placement carrier tail is retained as a named opaque carrier.

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

## `joint_origin_legacy_class_337_266_frame`

Spec §3.1 · layout: byte offsets · size: 389 B

Offsets are relative to the class-337 primary indexed placement header paired with class 266. The 212-byte tail is generation-specific.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 45 | 4 | `matrix_prefix` | `bytes[4]` | little | spec | stores 34 zero bytes, `0x01, 0x01, 0x00, 0x00` · value `[1, 1, 0, 0]` |
| 49 | 128 | `matrix` | `f64[16]` | little | spec | the matrix at offset 49 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..45` (34 B): Bytes 11 through 44 are zero.
- `177..389` (212 B): The generation-specific placement tail is retained as a named opaque carrier.

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

## `work_plane_direct_352_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 352 B

Offsets are relative to the primary indexed placement header (class 293 paired with class 261, or class 316 paired with class 260). The two-byte lane at offset 62 is opaque.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 55 | 1 | `marker` | `u8` | little | spec | store `0x01` at offset 55 · value `1` |
| 56 | 6 | `zero_run` | `bytes[6]` | little | spec | six zero bytes at offsets 56 through 61 |
| 62 | 2 | `opaque_u16` | `u16` | little | spec | an opaque little-endian u16 at offset 62 |
| 64 | 2 | `zero_pair` | `bytes[2]` | little | spec | two zero bytes at offsets 64 and 65 |
| 66 | 128 | `matrix` | `f64[16]` | little | spec | the matrix at offset 66 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..55` (44 B): Bytes 11 through 54 are zero.
- `194..352` (158 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_direct_363_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 363 B

Offsets are relative to the class-289 primary indexed placement header paired with class 258. The two-byte lane at offset 62 is opaque.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 55 | 1 | `marker` | `u8` | little | spec | store `0x01` at offset 55 · value `1` |
| 56 | 6 | `zero_run` | `bytes[6]` | little | spec | six zero bytes at offsets 56 through 61 |
| 62 | 2 | `opaque_u16` | `u16` | little | spec | an opaque little-endian u16 at offset 62 |
| 64 | 2 | `zero_pair` | `bytes[2]` | little | spec | two zero bytes at offsets 64 and 65 |
| 66 | 128 | `matrix` | `f64[16]` | little | spec | the matrix at offset 66 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..55` (44 B): Bytes 11 through 54 are zero.
- `194..363` (169 B): The placement carrier tail is retained as a named opaque carrier.

## `work_plane_direct_374_matrix_frame`

Spec §3.1 · layout: byte offsets · size: 374 B

Offsets are relative to the class-267 primary indexed placement header paired with class 258. The two-byte lane at offset 62 is opaque.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 55 | 1 | `marker` | `u8` | little | spec | store `0x01` at offset 55 · value `1` |
| 56 | 6 | `zero_run` | `bytes[6]` | little | spec | six zero bytes at offsets 56 through 61 |
| 62 | 2 | `opaque_u16` | `u16` | little | spec | an opaque little-endian u16 at offset 62 |
| 64 | 2 | `zero_pair` | `bytes[2]` | little | spec | two zero bytes at offsets 64 and 65 |
| 66 | 128 | `matrix` | `f64[16]` | little | spec | the matrix at offset 66 |

Unstated regions:

- `0..11` (11 B): The indexed placement header occupies the first eleven bytes.
- `11..55` (44 B): Bytes 11 through 54 are zero.
- `194..374` (180 B): The placement carrier tail is retained as a named opaque carrier.

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

## `offset_24_extrude_prologue`

Spec §3.1 · layout: byte offsets · size: 28 B

Offsets are relative to the Extrude primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/extrude.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 13 | `zero_run_13` | `bytes[13]` | little | spec | at primary-header offset 24 after thirteen zero bytes |
| 24 | 4 | `operation` | `u32` | little | spec | at primary-header offset 24 after thirteen zero bytes |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

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

## `legacy_class_338_two_sided_distance_extrude_frame`

Spec §3.1 · layout: byte offsets · size: 495 B

Offsets are relative to the class-338 primary indexed header. The paired class-262 header begins at offset 495. The class-local envelope contains a ten-byte null scope-and-scalar lane, shifted two-sided extent fields, and ten ordered references beginning at offset 286.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | stores u32 `1` at primary-header offset 20 · value `1` |
| 27 | 4 | `operation` | `u32` | little | spec | result operation: `1 = join` |
| 31 | 4 | `direction` | `u32` | little | spec | travel direction `2` (two sides) · value `2` |
| 35 | 4 | `face_extend` | `u32` | little | spec | face-extend value `0` · value `0` |
| 39 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal |
| 40 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind |
| 41 | 1 | `start_support` | `u8` | little | spec | start support |
| 45 | 24 | `profile_normal` | `f64[3]` | little | spec | finite unit profile normal |
| 69 | 70 | `class_local_envelope` | `bytes[70]` | little | spec | class-local nullable-selection and parameter envelope |
| 139 | 10 | `null_scope_scalar_lane` | `bytes[10]` | little | spec | null scope-and-scalar lane |
| 149 | 11 | `first_side_parameter_reference` | `bytes[11]` | little | spec | marked first-side parameter reference |
| 165 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent `1` (distance) · value `1` |
| 169 | 11 | `profile_group_reference` | `bytes[11]` | little | spec | marked profile construction-group reference |
| 188 | 4 | `second_side_extent` | `u32` | little | spec | second-side extent `1` (distance) · value `1` |
| 192 | 11 | `body_group_reference` | `bytes[11]` | little | spec | marked body construction-group reference |
| 203 | 76 | `guid` | `bytes[76]` | little | spec | LP-UTF16 GUID: 36 code units |
| 282 | 4 | `reference_count` | `u32` | little | spec | ordered reference count `10` · value `10` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding shifted-operation envelope are outside this fixed frame prefix.
- `24..27` (3 B): Three zero bytes separate the prefix constant from the result operation.
- `42..45` (3 B): Three zero bytes precede the profile normal.
- `160..165` (5 B): Five zero bytes separate the first-side parameter reference from the first-side extent.
- `180..188` (8 B): Eight zero bytes separate the profile-group reference from the second-side extent.
- `279..282` (3 B): Three zero bytes separate the GUID from the ordered reference count.
- `286..495` (209 B): The ten ordered reference entries and common scope tail precede the paired indexed header at offset 495.

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

## `class_296_261_one_sided_to_face_extrude_prefix`

Spec §3.1 · layout: byte offsets · size: 265 B

Offsets are relative to the class-296 primary indexed header. The paired header begins at +440, +462, or +473 for ordered reference counts 7, 9, or 10; the reference-count field is fixed at +261.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | common operation fields at offsets `+20` · value `1` |
| 24 | 2 | `zero_run_2` | `bytes[2]` | little | spec | `+24..+25` |
| 26 | 4 | `operation` | `u32` | little | spec | `+26` |
| 30 | 4 | `direction` | `u32` | little | spec | travel direction is `1` · value `1` |
| 34 | 4 | `face_extend` | `u32` | little | spec | face-extend option is `1` or `2` |
| 38 | 1 | `direction_reversed` | `u8` | little | spec | direction-reversal |
| 39 | 1 | `geometry_kind` | `u8` | little | spec | geometry-kind |
| 40 | 1 | `start_support` | `u8` | little | spec | start-support |
| 105 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent discriminator is `u32 2` at `+105` · value `2` |
| 257 | 4 | `second_side_extent` | `u32` | little | spec | second-side discriminator is `u32 0` at `+257 = reference_count - 4` · value `0` |
| 261 | 4 | `reference_count` | `u32` | little | spec | ordered reference count is at `+261` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.
- `41..105` (64 B): The class-specific envelope between the prologue and first-side extent is retained as opaque bytes.
- `109..257` (148 B): The side-reference and persistent-selection lanes precede the second-side discriminator.

## `class_296_261_symmetric_distance_extrude_prefix`

Spec §3.1 · layout: byte offsets · size: 275 B

Offsets are relative to the class-296 primary indexed header. The paired header begins at +450 for ordered reference count 7; the reference-count field is fixed at +271.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | `+20` · value `1` |
| 24 | 2 | `zero_run_2` | `bytes[2]` | little | spec | `+24..+25` |
| 26 | 4 | `operation` | `u32` | little | spec | `+26` |
| 30 | 4 | `direction` | `u32` | little | spec | direction `3` at `+30` · value `3` |
| 34 | 4 | `face_extend` | `u32` | little | spec | face-extend option `2` at `+34` · value `2` |
| 38 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal at `+38` |
| 39 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind at `+39` |
| 40 | 1 | `start_support` | `u8` | little | spec | start support at `+40` |
| 128 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent discriminator is `u32 1` at `+128` · value `1` |
| 267 | 4 | `second_side_extent` | `u32` | little | spec | second-side discriminator is `u32 0` at `+267 = reference_count - 4` · value `0` |
| 271 | 4 | `reference_count` | `u32` | little | spec | ordered reference count is `7` at `+271` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.
- `41..128` (87 B): The class-specific reference lanes precede the symmetric extent discriminators.
- `132..267` (135 B): The remaining class-specific reference and selection lanes precede the second-side discriminator.

## `class_296_261_two_sided_to_faces_extrude_prefix`

Spec §3.1 · layout: byte offsets · size: 295 B

Offsets are relative to the class-296 primary indexed header. The paired header begins at +536 for ordered reference count 13; the reference-count field is fixed at +291.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | `+20` · value `1` |
| 24 | 2 | `zero_run_2` | `bytes[2]` | little | spec | `+24..+25` |
| 26 | 4 | `operation` | `u32` | little | spec | `+26` |
| 30 | 4 | `direction` | `u32` | little | spec | direction `2` at `+30` · value `2` |
| 34 | 4 | `face_extend` | `u32` | little | spec | face-extend option `1` or `2` at `+34` |
| 38 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal at `+38` |
| 39 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind at `+39` |
| 40 | 1 | `start_support` | `u8` | little | spec | start support at `+40` |
| 41 | 3 | `zero_run_3_after_start` | `bytes[3]` | little | spec | Three zero bytes at `+41..+43` |
| 44 | 24 | `profile_normal` | `f64[3]` | little | spec | unit profile normal at `+44` |
| 68 | 47 | `reference_slots` | `bytes[47]` | little | spec | seven nullable slots at `+68` |
| 115 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent discriminator is `u32 2` at `+115` · value `2` |
| 287 | 4 | `second_side_extent` | `u32` | little | spec | second-side discriminator is `u32 0` at `+287 = reference_count - 4` · value `0` |
| 291 | 4 | `reference_count` | `u32` | little | spec | ordered reference count is `13` at `+291` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.
- `119..287` (168 B): The class-specific face-selection, parameter, and historical-reference lanes precede the second-side discriminator.

## `class_296_261_legacy_extrude_prefix_scalar_at_54`

Spec §3.1 · layout: byte offsets · size: 129 B

Offsets are relative to the class-296 primary indexed header. This prefix is shared by the 515-byte one-sided-to-face and 483-byte one-sided-distance legacy envelopes; the scalar lane is at +54 in this variant.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | u32 `1` at `+20` · value `1` |
| 24 | 1 | `zero_before_reference` | `u8` | little | spec | a zero byte at `+24` · value `0` |
| 25 | 11 | `reference` | `bytes[11]` | little | spec | a marked reference at `+25` with six trailing zero bytes through `+35` |
| 36 | 4 | `operation` | `u32` | little | spec | the result operation at `+36` |
| 40 | 4 | `direction` | `u32` | little | spec | direction `1` at `+40` · value `1` |
| 44 | 4 | `face_extend` | `u32` | little | spec | face-extend option `1` at `+44` |
| 48 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal at `+48` |
| 49 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind at `+49` |
| 50 | 1 | `start_support` | `u8` | little | spec | start support at `+50` |
| 51 | 3 | `zero_after_start` | `bytes[3]` | little | spec | three zero bytes at `+51..+53` |
| 54 | 8 | `profile_scalar_at_54` | `f64` | little | spec | its profile scalar at `+54` |
| 62 | 16 | `zero_after_scalar_at_54` | `bytes[16]` | little | spec | `+62..+77` are zero |
| 78 | 47 | `reference_slots` | `bytes[47]` | little | spec | seven nullable slots at `+78` |
| 125 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent discriminator is `u32 2` at `+125` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.

## `class_296_261_legacy_extrude_prefix_scalar_at_70`

Spec §3.1 · layout: byte offsets · size: 129 B

Offsets are relative to the class-296 primary indexed header. This prefix uses the alternate scalar lane at +70 and is admitted only with the exact legacy class, frame, and reference-count tuple.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 4 | `prefix_constant` | `u32` | little | spec | u32 `1` at `+20` · value `1` |
| 24 | 1 | `zero_before_reference` | `u8` | little | spec | a zero byte at `+24` · value `0` |
| 25 | 11 | `reference` | `bytes[11]` | little | spec | a marked reference at `+25` with six trailing zero bytes through `+35` |
| 36 | 4 | `operation` | `u32` | little | spec | the result operation at `+36` |
| 40 | 4 | `direction` | `u32` | little | spec | direction `1` at `+40` · value `1` |
| 44 | 4 | `face_extend` | `u32` | little | spec | face-extend option `2` at `+44` |
| 48 | 1 | `direction_reversed` | `u8` | little | spec | direction reversal at `+48` |
| 49 | 1 | `geometry_kind` | `u8` | little | spec | geometry kind at `+49` |
| 50 | 1 | `start_support` | `u8` | little | spec | start support at `+50` |
| 51 | 3 | `zero_after_start` | `bytes[3]` | little | spec | three zero bytes at `+51..+53` |
| 54 | 16 | `zero_before_scalar_at_70` | `bytes[16]` | little | spec | `+54..+69` are zero |
| 70 | 8 | `profile_scalar_at_70` | `f64` | little | spec | its profile scalar at `+70` |
| 78 | 47 | `reference_slots` | `bytes[47]` | little | spec | seven nullable slots at `+78` |
| 125 | 4 | `first_side_extent` | `u32` | little | spec | first-side extent discriminator is `u32 1` at `+125` |

Unstated regions:

- `0..20` (20 B): The indexed header and preceding scope envelope are outside this fixed prefix.

## `class_296_261_legacy_one_sided_to_face_tail`

Spec §3.1 · layout: byte offsets · size: 285 B

Offsets are relative to the class-296 primary indexed header. The ordered references, history transition, and paired class-261 header follow this fixed tail; the paired header begins at +515.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 277 | 4 | `second_side_extent` | `u32` | little | spec | the second-side discriminator is `u32 0` at `+277 = reference_count - 4` · value `0` |
| 281 | 4 | `reference_count` | `u32` | little | spec | the ordered reference count is `12` at `+281` · value `12` |

Unstated regions:

- `0..277` (277 B): The class-specific side-reference and history lanes precede the second-side extent and count.

## `class_296_261_legacy_one_sided_distance_tail`

Spec §3.1 · layout: byte offsets · size: 275 B

Offsets are relative to the class-296 primary indexed header. The ordered references, history transition, and paired class-261 header follow this fixed tail; the paired header begins at +483.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 267 | 4 | `second_side_extent` | `u32` | little | spec | the second-side discriminator is `u32 0` at `+267 = reference_count - 4` · value `0` |
| 271 | 4 | `reference_count` | `u32` | little | spec | the ordered reference count is `10` at `+271` · value `10` |

Unstated regions:

- `0..267` (267 B): The class-specific side-reference and history lanes precede the second-side extent and count.

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
| 38 | 4 | `compact_trailer` | `bytes[4]` | little | spec | the four bytes `00 00 00 01` |

Unstated regions:

- `5..38` (33 B): The compact scalar lanes use the same offsets as the standard construction tail.

## `thread_standard_legacy_construction_tail`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the first byte after the third LP-UTF16 string. This class-specific tail keeps the standard one-group reference table while using the compact scalar and no-reference trailer lanes.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `construction_marker` | `bytes[5]` | little | spec | Its construction marker is `01 01 00 00 00` |
| 5 | 8 | `major_diameter` | `f64` | little | spec | Major diameter is at marker-relative offset 5 |
| 13 | 8 | `minor_diameter` | `f64` | little | spec | minor diameter at 13 |
| 21 | 1 | `pitch_marker` | `u8` | little | spec | its scalar lanes use pitch marker `0` |
| 22 | 8 | `pitch` | `f64` | little | spec | pitch at 22 |
| 30 | 8 | `pitch_diameter` | `f64` | little | spec | pitch diameter at 30 |
| 38 | 4 | `legacy_trailer` | `bytes[4]` | little | spec | its no-reference trailer is `00 00 00 01` at relative offset 38 |

## `thread_compact_legacy_construction_tail`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the first byte after the third LP-UTF16 string. This class-specific tail keeps the compact paired reference table while using the legacy scalar and no-reference trailer lanes.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `construction_marker` | `bytes[5]` | little | spec | Its construction marker is `01 01 00 00 00` |
| 5 | 8 | `major_diameter` | `f64` | little | spec | Major diameter is at marker-relative offset 5 |
| 13 | 8 | `minor_diameter` | `f64` | little | spec | minor diameter at 13 |
| 21 | 1 | `pitch_marker` | `u8` | little | spec | its scalar lanes use pitch marker `0` |
| 22 | 8 | `pitch` | `f64` | little | spec | pitch at 22 |
| 30 | 8 | `pitch_diameter` | `f64` | little | spec | pitch diameter at 30 |
| 38 | 4 | `legacy_trailer` | `bytes[4]` | little | spec | its no-reference trailer is `00 00 00 01` at relative offset 38 |

## `thread_compact_reference_construction_tail`

Spec §3.1 · layout: byte offsets · size: 49 B

Offsets are relative to the first byte after the third LP-UTF16 string.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `construction_marker` | `bytes[5]` | little | spec | `01 02 00 00 00` in the compact form |
| 38 | 1 | `reference_marker` | `u8` | little | spec | byte `01`, a nonzero u32 record index |
| 39 | 4 | `record_index` | `u32` | little | spec | byte `01`, a nonzero u32 record index |
| 43 | 6 | `zero_tail` | `bytes[6]` | little | spec | and six zero bytes |

Unstated regions:

- `5..38` (33 B): The compact scalar lanes use the same offsets as the standard construction tail.

## `sketch_txt_tag_post_reference_members`

Spec §3.1 · layout: byte offsets · size: 15 B

Offsets start after the counted post-string reference run.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `preweight` | `bytes[3]` | little | spec | Its text string is followed by a u32-counted reference run · value `[0, 1, 0]` |
| 3 | 4 | `font_weight` | `i32` | little | spec | Its text string is followed by a u32-counted reference run |
| 7 | 8 | `cached_natural_width_cm` | `f64` | little | spec | Its text string is followed by a u32-counted reference run |

## `sketch_text_legacy_presentation_suffix`

Spec §3.1 · layout: byte offsets · size: 30 B

Offsets start at the thirty-byte inherited presentation suffix before the owning-sketch reference.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `prefix` | `u32` | little | spec | The legacy text presentation suffix is thirty bytes · value `0` |
| 4 | 1 | `present` | `u8` | little | spec | The legacy text presentation suffix is thirty bytes · value `1` |
| 5 | 20 | `default_components` | `f32[5]` | little | spec | The legacy text presentation suffix is thirty bytes |
| 25 | 4 | `zero_word` | `u32` | little | spec | The legacy text presentation suffix is thirty bytes · value `0` |
| 29 | 1 | `zero_byte` | `u8` | little | spec | The legacy text presentation suffix is thirty bytes · value `0` |

## `sketch_tangency_class_flags`

Spec §3.1 · layout: byte offsets · size: 3 B

Offsets start after the tangency property block. Endpoint roles require a bounded endpoint contact in semantic return order.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `first_curve_start` | `bool8` | little | spec | The tangency subclass flags refer |
| 1 | 1 | `second_curve_start` | `bool8` | little | spec | The tangency subclass flags refer |
| 2 | 1 | `positive_tangent_dot` | `bool8` | little | spec | The tangency subclass flags refer |

## `sketch_rectangular_pattern_class_prefix`

Spec §3.1 · layout: byte offsets · size: 7 B

Offsets start after the rectangular-pattern property block, before the variable direction slots and pattern tables.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `uses_extent` | `bool8` | little | spec | The rectangular-pattern reference run |
| 1 | 1 | `direction_one_symmetric` | `bool8` | little | spec | The rectangular-pattern reference run |
| 2 | 1 | `unassigned_flag` | `bool8` | little | spec | The rectangular-pattern reference run |
| 3 | 4 | `direction_slot_count` | `u32` | little | spec | The rectangular-pattern reference run |

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

## `edge_flange_class286_two_sided_per_edge_fixed_operation`

Spec §3.1 · layout: byte offsets · size: 305 B

Offsets are relative to the primary scope header. The fixed operation fields close at the second marked role-0x08 group reference; the paired header follows the 801-byte primary frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 96 | 4 | `bend_position` | `u32` | little | spec | bend position at `96` |
| 100 | 4 | `edge_count` | `u32` | little | spec | edge count `2` at `100` |
| 104 | 11 | `edge_wrapper_one_reference` | `bytes[11]` | little | spec | wrapper references at `104` and `115` |
| 115 | 11 | `edge_wrapper_two_reference` | `bytes[11]` | little | spec | wrapper references at `104` and `115` |
| 126 | 11 | `settings_reference` | `bytes[11]` | little | spec | settings at `126` |
| 137 | 4 | `height_datum` | `u32` | little | spec | height datum at `137` |
| 141 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | angle owner at `141` |
| 152 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | height owner at `152` |
| 163 | 4 | `reference_side` | `u32` | little | spec | reference side at `163` |
| 169 | 8 | `inside_bend_radius` | `f64` | little | spec | positive finite f64 inside bend radius in centimetres at `169` |
| 177 | 4 | `result_count` | `u32` | little | spec | Result count `5` is at `177` |
| 181 | 11 | `result_one_reference` | `bytes[11]` | little | spec | five 15-byte result records start at `181` |
| 192 | 4 | `result_one_trailer` | `u32` | little | spec | trailers `1` at `192`, `207`, `222`, `237`, and `0` at `252` |
| 196 | 11 | `result_two_reference` | `bytes[11]` | little | spec | five 15-byte result records start at `181` |
| 207 | 4 | `result_two_trailer` | `u32` | little | spec | trailers `1` at `192`, `207`, `222`, `237`, and `0` at `252` |
| 211 | 11 | `result_three_reference` | `bytes[11]` | little | spec | five 15-byte result records start at `181` |
| 222 | 4 | `result_three_trailer` | `u32` | little | spec | trailers `1` at `192`, `207`, `222`, `237`, and `0` at `252` |
| 226 | 11 | `result_four_reference` | `bytes[11]` | little | spec | five 15-byte result records start at `181` |
| 237 | 4 | `result_four_trailer` | `u32` | little | spec | trailers `1` at `192`, `207`, `222`, `237`, and `0` at `252` |
| 241 | 11 | `result_five_reference` | `bytes[11]` | little | spec | five 15-byte result records start at `181` |
| 252 | 4 | `result_five_trailer` | `u32` | little | spec | trailers `1` at `192`, `207`, `222`, `237`, and `0` at `252` |
| 256 | 4 | `result_separator` | `u32` | little | spec | A u32 value `1` at `256` precedes the aggregate-group reference at `260` |
| 260 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | aggregate-group reference at `260` |
| 283 | 11 | `edge_group_one_reference` | `bytes[11]` | little | spec | group references start at `283` and `294` |
| 294 | 11 | `edge_group_two_reference` | `bytes[11]` | little | spec | group references start at `283` and `294` |

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

Offsets are relative to the transform record's primary indexed header. The class tags are the admission discriminator; the class-447 form uses paired class 263, the class-456 form uses paired class 258, and the class-452 form uses paired class 262 and form word 5; the same-index paired header follows at offset 253.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | transform records are 253 bytes to their same-index paired headers |
| 43 | 4 | `form` | `u32` | little | spec | offset 43 stores u32 form `1` or `5` |
| 47 | 1 | `reserved_zero` | `u8` | little | spec | offset 47 is zero |
| 48 | 128 | `transform` | `f64[16]` | little | spec | sixteen row-major f64 values begin at offset 48 |

Unstated regions:

- `11..43` (32 B): Offsets 11 through 42 are zero.
- `176..253` (77 B): The transform record's native tail precedes the same-index paired header at offset 253.

## `move_transform_frame_254`

Spec §3.1 · layout: byte offsets · size: 254 B

Offsets are relative to the transform record's primary indexed header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | class `368` transform records are 254 bytes |
| 43 | 4 | `form` | `u32` | little | spec | offset 43 stores u32 form `1` or `5` |
| 47 | 1 | `reserved_zero` | `u8` | little | spec | offset 47 is zero |
| 48 | 128 | `transform` | `f64[16]` | little | spec | sixteen row-major f64 values begin at offset 48 |

Unstated regions:

- `11..43` (32 B): Offsets 11 through 42 are zero.
- `176..254` (78 B): The transform record's tail precedes the same-index paired header at offset 254.

## `move_transform_frame_273`

Spec §3.1 · layout: byte offsets · size: 273 B

Offsets are relative to the transform record's primary indexed header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | transform records are 273 bytes |
| 43 | 4 | `form` | `u32` | little | spec | The 273-byte frames use form word `1` · value `1` |
| 47 | 1 | `reserved_zero` | `u8` | little | spec | offset 47 is zero |
| 48 | 128 | `transform` | `f64[16]` | little | spec | sixteen row-major f64 values begin at offset 48 |

Unstated regions:

- `11..43` (32 B): Offsets 11 through 42 are zero.
- `176..273` (97 B): The transform record's tail precedes the same-index paired header at offset 273.

## `move_transform_frame_274`

Spec §3.1 · layout: byte offsets · size: 274 B

Offsets are relative to the transform record's primary indexed header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Class `349` transform records are 254 or 274 bytes |
| 43 | 4 | `form` | `u32` | little | spec | offset 43 stores u32 form `1` or `5` |
| 47 | 1 | `reserved_zero` | `u8` | little | spec | offset 47 is zero |
| 48 | 128 | `transform` | `f64[16]` | little | spec | sixteen row-major f64 values begin at offset 48 |

Unstated regions:

- `11..43` (32 B): Offsets 11 through 42 are zero.
- `176..274` (98 B): The transform record's tail precedes the same-index paired header at offset 274.

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

## `component_insert_identity_scope_compact`

Spec §3.1 · layout: byte offsets · size: 261 B

Offsets are relative to the primary indexed header. The paired indexed header begins at offset 261 for every admitted class pair.

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

## `component_insert_identity_scope_shifted_prefix`

Spec §3.1 · layout: byte offsets · size: 122 B

Offsets are relative to the primary indexed header. The generation-specific scope tail begins at offset 122.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | 257- and 267-byte shifted identity frames |
| 21 | 8 | `occurrence_identity` | `u64` | little | spec | the occurrence identity at offset 21 |
| 33 | 1 | `relation_marker` | `u8` | little | spec | the marked relation at offset 33 |
| 34 | 4 | `relation_record_index` | `u32` | little | spec | the marked relation at offset 33 |
| 44 | 2 | `identity_markers` | `bytes[2]` | little | spec | bytes `01 01` at offsets 44 and 45 |
| 46 | 4 | `null_guid_code_unit_count` | `u32` | little | spec | the null-GUID code-unit count at offset 46 |
| 50 | 72 | `null_guid_utf16_payload` | `bytes[72]` | little | spec | its UTF-16 payload at offset 50 |

Unstated regions:

- `11..21` (10 B): Ten zero bytes occupy offsets 11 through 20.
- `29..33` (4 B): Four zero bytes occupy offsets 29 through 32.
- `38..44` (6 B): Six zero bytes occupy offsets 38 through 43.

## `component_insert_matrix_scope_414_264_prefix`

Spec §3.1 · layout: byte offsets · size: 254 B

Offsets are relative to the primary indexed header. The 261-byte identity tail resumes at offset 254 after its 128-byte shift.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The 389-byte matrix frame |
| 20 | 1 | `prologue_marker` | `u8` | little | spec | uses the 261-byte prologue |
| 21 | 4 | `prologue_value` | `u32` | little | spec | uses the 261-byte prologue |
| 25 | 8 | `occurrence_identity` | `u64` | little | spec | uses the 261-byte prologue |
| 37 | 1 | `relation_marker` | `u8` | little | spec | uses the 261-byte prologue through the relation |
| 38 | 4 | `relation_record_index` | `u32` | little | spec | uses the 261-byte prologue through the relation |
| 48 | 2 | `matrix_markers` | `bytes[2]` | little | spec | bytes `01 00` at offsets 48 and 49 |
| 50 | 128 | `transform` | `f64[16]` | little | spec | a row-major rigid transform at offset 50 |
| 178 | 4 | `null_guid_code_unit_count` | `u32` | little | spec | the null-GUID code-unit count at offset 178 |
| 182 | 72 | `null_guid_utf16_payload` | `bytes[72]` | little | spec | its UTF-16 payload at offset 182 |

Unstated regions:

- `11..20` (9 B): Nine zero bytes occupy offsets 11 through 19.
- `33..37` (4 B): Four zero bytes occupy offsets 33 through 36.
- `42..48` (6 B): Six zero bytes occupy offsets 42 through 47.

## `component_insert_relation_345_57`

Spec §3.1 · layout: byte offsets · size: 57 B

Offsets are relative to the primary class-345 indexed header. The paired class-258 header begins at offset 57.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | class-`345`/class-`258` relation |
| 21 | 1 | `first_marker` | `u8` | little | spec | the carrier reference at offset 22 · value `1` |
| 22 | 4 | `first_carrier_record_index` | `u32` | little | spec | the carrier reference at offset 22 |
| 34 | 1 | `second_marker` | `u8` | little | spec | the child reference at offset 35 · value `1` |
| 35 | 4 | `second_child_record_index` | `u32` | little | spec | the child reference at offset 35 |
| 46 | 1 | `scope_marker` | `u8` | little | spec | the scope back-reference at offset 47 · value `1` |
| 47 | 4 | `scope_record_index` | `u32` | little | spec | the scope back-reference at offset 47 |

Unstated regions:

- `11..21` (10 B): Ten zero bytes occupy offsets 11 through 20.
- `26..34` (8 B): Eight zero bytes occupy offsets 26 through 33.
- `39..46` (7 B): Seven zero bytes occupy offsets 39 through 45.
- `51..57` (6 B): Six zero bytes occupy offsets 51 through 56.

## `component_insert_relation_348_57`

Spec §3.1 · layout: byte offsets · size: 57 B

Offsets are relative to the primary class-348 indexed header. The paired class-266 header begins at offset 57.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | a 57-byte class-`348`/class-`266` relation |
| 21 | 1 | `first_marker` | `u8` | little | spec | the same three marked references and zero tails · value `1` |
| 22 | 4 | `first_carrier_record_index` | `u32` | little | spec | Its first reference names a class-`341` grouped identity carrier |
| 34 | 1 | `second_marker` | `u8` | little | spec | the same three marked references and zero tails · value `1` |
| 35 | 4 | `second_record_index` | `u32` | little | spec | the same three marked references and zero tails |
| 46 | 1 | `scope_marker` | `u8` | little | spec | the same three marked references and zero tails · value `1` |
| 47 | 4 | `scope_record_index` | `u32` | little | spec | the same three marked references and zero tails |

Unstated regions:

- `11..21` (10 B): Ten zero bytes occupy offsets 11 through 20.
- `26..34` (8 B): Eight zero bytes follow the first relation member.
- `39..46` (7 B): Seven zero bytes follow the middle relation member.
- `51..57` (6 B): Six zero bytes occupy offsets 51 through 56.

## `component_insert_relation_child_393_58`

Spec §3.1 · layout: byte offsets · size: 58 B

Offsets are relative to the primary class-393 indexed header. The paired header begins at offset 58.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | class-`393` child record |
| 31 | 1 | `relation_marker` | `u8` | little | spec | stores the relation back-reference at offset 32 · value `1` |
| 32 | 4 | `relation_record_index` | `u32` | little | spec | stores the relation back-reference at offset 32 |
| 42 | 8 | `opaque_token` | `u64` | little | spec | carries an opaque u64 token |

Unstated regions:

- `11..31` (20 B): Twenty zero bytes occupy offsets 11 through 30.
- `36..42` (6 B): Six zero bytes occupy offsets 36 through 41.
- `50..58` (8 B): Eight zero bytes occupy offsets 50 through 57.

## `component_insert_scope_283_262_257`

Spec §3.1 · layout: byte offsets · size: 257 B

Offsets are relative to the primary class-283 indexed header. The paired class-262 indexed header begins at offset 257. The final fixed tail contains generation state bytes between the typed lanes and the paired header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | `byte_offset` is the primary class-`283` indexed header |
| 21 | 8 | `occurrence_identity` | `u64` | little | spec | an occurrence identity u64 at offset 21 |
| 33 | 1 | `relation_marker` | `u8` | little | spec | a marked relation reference at offset 33 · value `1` |
| 34 | 4 | `relation_record_index` | `u32` | little | spec | its u32 value at offset 34 |
| 44 | 2 | `identity_markers` | `bytes[2]` | little | spec | The 257-byte form stores `01 01` at offsets 44 and 45 · value `[1, 1]` |
| 46 | 4 | `null_guid_code_unit_count` | `u32` | little | spec | an LP-UTF16 null component GUID with code-unit count 36 at offset 46 · value `36` |
| 50 | 72 | `null_guid_utf16_payload` | `bytes[72]` | little | spec | payload at offset 50 |
| 125 | 4 | `reference_count` | `u32` | little | spec | one ordered reference at offset 125 · value `1` |
| 129 | 1 | `reference_marker` | `u8` | little | spec | marker 1 at offset 129 · value `1` |
| 130 | 4 | `reference_record_index` | `u32` | little | spec | the relation index at offset 130 |
| 140 | 4 | `history_state_id` | `u32` | little | spec | the current history-state identity `0xffffffff` at offset 140 · value `4294967295` |
| 144 | 4 | `kind_code_unit_count` | `u32` | little | spec | Its kind count is 16 at offset 144 · value `16` |
| 148 | 32 | `kind_utf16_payload` | `bytes[32]` | little | spec | its `Component Insert` payload starts at offset 148 |
| 180 | 4 | `feature_ordinal` | `u32` | little | spec | The feature ordinal is at offset 180 |
| 211 | 4 | `previous_history_state_id` | `u32` | little | spec | the preceding history-state identity is at offset 211 |

Unstated regions:

- `11..21` (10 B): Ten zero bytes occupy offsets 11 through 20.
- `29..33` (4 B): Four zero bytes occupy offsets 29 through 32.
- `38..44` (6 B): Six zero bytes occupy offsets 38 through 43.
- `122..125` (3 B): Three zero bytes occupy offsets 122 through 124.
- `134..140` (6 B): Six zero bytes occupy offsets 134 through 139.
- `184..211` (27 B): The fixed generation tail precedes the preceding history-state identity.
- `215..257` (42 B): The remaining fixed generation tail ends at the paired class-262 header.

## `component_insert_scope_283_262_385`

Spec §3.1 · layout: byte offsets · size: 385 B

Offsets are relative to the primary class-283 indexed header. The paired class-262 indexed header begins at offset 385. The final fixed tail contains generation state bytes between the typed lanes and the paired header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | `byte_offset` is the primary class-`283` indexed header |
| 21 | 8 | `occurrence_identity` | `u64` | little | spec | an occurrence identity u64 at offset 21 |
| 33 | 1 | `relation_marker` | `u8` | little | spec | a marked relation reference at offset 33 · value `1` |
| 34 | 4 | `relation_record_index` | `u32` | little | spec | its u32 value at offset 34 |
| 44 | 2 | `transform_markers` | `bytes[2]` | little | spec | The 385-byte form stores `01 00` at offsets 44 and 45 · value `[1, 0]` |
| 46 | 128 | `transform` | `f64[16]` | little | spec | a row-major rigid transform of 16 little-endian f64 values at offset 46 |
| 174 | 4 | `null_guid_code_unit_count` | `u32` | little | spec | the null component GUID count at offset 174 · value `36` |
| 178 | 72 | `null_guid_utf16_payload` | `bytes[72]` | little | spec | payload at offset 178 |
| 253 | 4 | `reference_count` | `u32` | little | spec | one ordered reference at offset 253 · value `1` |
| 257 | 1 | `reference_marker` | `u8` | little | spec | marker 1 at offset 257 · value `1` |
| 258 | 4 | `reference_record_index` | `u32` | little | spec | the relation index at offset 258 |
| 268 | 4 | `history_state_id` | `u32` | little | spec | the current history-state identity `0xffffffff` at offset 268 · value `4294967295` |
| 272 | 4 | `kind_code_unit_count` | `u32` | little | spec | Its kind count is 16 at offset 272 · value `16` |
| 276 | 32 | `kind_utf16_payload` | `bytes[32]` | little | spec | its `Component Insert` payload starts at offset 276 |
| 308 | 4 | `feature_ordinal` | `u32` | little | spec | The feature ordinal is at offset 308 |
| 339 | 4 | `previous_history_state_id` | `u32` | little | spec | the preceding history-state identity is at offset 339 |

Unstated regions:

- `11..21` (10 B): Ten zero bytes occupy offsets 11 through 20.
- `29..33` (4 B): Four zero bytes occupy offsets 29 through 32.
- `38..44` (6 B): Six zero bytes occupy offsets 38 through 43.
- `250..253` (3 B): Three zero bytes occupy offsets 250 through 252.
- `262..268` (6 B): Six zero bytes occupy offsets 262 through 267.
- `312..339` (27 B): The fixed generation tail precedes the preceding history-state identity.
- `343..385` (42 B): The remaining fixed generation tail ends at the paired class-262 header.

## `component_insert_carrier_334_prefix`

Spec §3.1 · layout: byte offsets · size: not stated

This size-less byte record describes the fixed carrier prefix through the first direct role code unit. The direct role, discriminator tail, and following identity are variable-length suffix fields.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A class-`334` carrier stores |
| 38 | 4 | `component_identity` | `u32` | little | spec | a 36-code-unit component identity as an LP-UTF16 field at offset 38 · value `36` |
| 42 | 72 | `component_identity_utf16` | `bytes[72]` | little | spec | a 36-code-unit component identity as an LP-UTF16 field at offset 38 |
| 159 | 2 | `neutron_role` | `u16` | little | spec | a direct UTF-16LE neutron-role payload at offset 159 |

Unstated regions:

- `11..38` (27 B): The fixed prefix reaches the component identity count at offset 38.
- `114..159` (45 B): The fixed prefix ends immediately before the direct neutron-role payload.

## `component_insert_grouped_identity_carrier`

Spec §3.1 · layout: byte offsets · size: 695 B

Offsets are relative to the primary class-380, class-382, or compact class-369 grouped identity carrier header. The relation header begins at offset 695. Every GUID field has 36 code units or ASCII bytes.

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
| 231 | 10 | `metadata_marker` | `bytes[10]` | little | spec | The class-`382` and class-`380` forms use `00 01 00 00 00 00 01 00 00 00` after the role |
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

## `component_insert_grouped_identity_carrier_341`

Spec §3.1 · layout: byte offsets · size: not stated

This size-less record gives the fixed prefix of the variable-length class-341 carrier. The role length is 36 through 256 code units. The first two suffix markers and the repeated occurrence identity are defined in the specification; the relation header begins at the closure.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | the grouped prefix through the occurrence identity |
| 19 | 1 | `carrier_marker` | `u8` | little | spec | The carrier has the grouped prefix through the occurrence identity · value `1` |
| 20 | 4 | `carrier_value` | `u32` | little | spec | the grouped prefix through the occurrence identity · value `1` |
| 24 | 1 | `identity_marker` | `u8` | little | spec | the grouped prefix through the occurrence identity · value `1` |
| 25 | 8 | `occurrence_identity` | `u64` | little | spec | the carrier's occurrence identity u64 |
| 33 | 1 | `reference_marker` | `u8` | little | spec | the grouped prefix through the occurrence identity · value `1` |
| 38 | 76 | `first_component_guid` | `bytes[76]` | little | spec | the component, type, and role fields above |
| 114 | 1 | `first_component_separator` | `u8` | little | spec | one zero separator byte · value `0` |
| 115 | 40 | `first_type_guid` | `bytes[40]` | little | spec | an LP-ASCII type GUID |
| 155 | 4 | `role_code_unit_count` | `u32` | little | spec | Its role is an LP-UTF16 value of 36 through 256 code units |

Unstated regions:

- `11..19` (8 B): Eight zero bytes occupy offsets 11 through 18.
- `34..38` (4 B): Four zero bytes occupy offsets 34 through 37.

## `derived_instance_scope_279_261`

Spec §3.1 · layout: byte offsets · size: 443 B

Offsets are relative to the primary indexed header. The paired class-261 header begins at offset 443; the frame carries one fixed prologue reference, one ordered relation reference, and a row-major rigid transform.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | `byte_offset` anchors its primary indexed header |
| 21 | 1 | `reference_marker` | `u8` | little | spec | A marked reference at offset 21 · value `1` |
| 22 | 4 | `reference_record_index` | `u32` | little | spec | carries `reference_record_index` at offset 22 |
| 32 | 4 | `reference_count` | `u32` | little | spec | The scope stores u32 `1` at offset 32 · value `1` |
| 36 | 11 | `relation_reference` | `bytes[11]` | little | spec | a marked relation reference at offset 36 |
| 48 | 128 | `transform` | `f64[16]` | little | spec | a row-major rigid transform of sixteen little-endian f64 values at offset 48 |

Unstated regions:

- `11..21` (10 B): Ten zero bytes occupy offsets 11 through 20.
- `26..32` (6 B): Six zero bytes follow the fixed prologue reference.
- `47..48` (1 B): One zero byte separates the ordered relation reference from the transform.
- `176..443` (267 B): The remaining fixed scope tail ends at the paired class-261 header.

## `derived_instance_relation_310_57`

Spec §3.1 · layout: byte offsets · size: 57 B

Offsets are relative to the class-310 relation's primary indexed header. The first member names the class-380 occurrence carrier and the third member names the DerivedInstance scope.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Ten zero bytes follow its indexed header |
| 21 | 1 | `carrier_marker` | `u8` | little | spec | its marked fields start at offsets 21, 34, and 46 · value `1` |
| 22 | 4 | `carrier_record_index` | `u32` | little | spec | The first relation member names the earlier class-`380` component-occurrence carrier |
| 34 | 1 | `middle_marker` | `u8` | little | spec | its marked fields start at offsets 21, 34, and 46 · value `1` |
| 35 | 4 | `middle_record_index` | `u32` | little | spec | its marked fields start at offsets 21, 34, and 46 |
| 46 | 1 | `scope_marker` | `u8` | little | spec | its marked fields start at offsets 21, 34, and 46 · value `1` |
| 47 | 4 | `scope_record_index` | `u32` | little | spec | the third names the DerivedInstance scope |

Unstated regions:

- `11..21` (10 B): Ten zero bytes occupy offsets 11 through 20.
- `26..34` (8 B): Eight zero bytes follow the first relation member.
- `39..46` (7 B): Seven zero bytes follow the middle relation member.
- `51..57` (6 B): Six zero bytes follow the scope back-reference.

## `canvas_geometry_payload`

Spec §3.1 · layout: byte offsets · size: 77 B

Offsets are relative to the Canvas geometry payload at geometry-record offset 69.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `opacity` | `f32` | little | spec | Its first four bytes are a normalized finite f32 opacity followed by one zero byte. |
| 4 | 1 | `reserved_zero` | `u8` | little | spec | Its first four bytes are a normalized finite f32 opacity followed by one zero byte. |
| 5 | 24 | `origin_centimetres` | `f64[3]` | little | spec | Three consecutive f64 triples begin at payload offsets 5, 29, and 53. |
| 29 | 24 | `u_axis` | `f64[3]` | little | spec | Three consecutive f64 triples begin at payload offsets 5, 29, and 53. |
| 53 | 24 | `v_axis` | `f64[3]` | little | spec | Three consecutive f64 triples begin at payload offsets 5, 29, and 53. |

## `canvas_geometry_prologue`

Spec §3.1 · layout: byte offsets · size: 15 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 10 | `zero_run_10` | `bytes[10]` | little | spec | Its first ten bytes and bytes 11 through 13 are zero, byte 10 is zero or one, and byte 14 is the Canvas visibility Boolean. |
| 10 | 1 | `first_flag` | `u8` | little | spec | Its first ten bytes and bytes 11 through 13 are zero, byte 10 is zero or one, and byte 14 is the Canvas visibility Boolean. |
| 11 | 3 | `zero_run_3` | `bytes[3]` | little | spec | Its first ten bytes and bytes 11 through 13 are zero, byte 10 is zero or one, and byte 14 is the Canvas visibility Boolean. |
| 14 | 1 | `visibility` | `u8` | little | spec | Its first ten bytes and bytes 11 through 13 are zero, byte 10 is zero or one, and byte 14 is the Canvas visibility Boolean. |

## `canvas_geometry_prefix`

Spec §3.1 · layout: byte offsets · size: 217 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The referenced geometry record stores a 15-byte prologue after its eleven-byte indexed header. |
| 11 | 15 | `geometry_prologue` | `bytes[15]` | little | spec | The referenced geometry record stores a 15-byte prologue after its eleven-byte indexed header. |
| 26 | 32 | `first_boundary` | `f64[4]` | little | spec | Four f64 values at record offsets 26, 34, 42, and 50 form the first plane-local boundary segment. |
| 58 | 11 | `plane_reference` | `bytes[11]` | little | spec | A marked supporting-plane entity suffix occurs at offset 58 with six trailing zero bytes. |
| 69 | 77 | `geometry_payload` | `bytes[77]` | little | spec | A 77-byte fixed payload occupies offsets 69 through 145. |
| 146 | 11 | `scope_reference` | `bytes[11]` | little | spec | Marked references at record offsets 146, 157, and 169 name the Canvas scope, owning component entity suffix, and image-asset record; their trailing fields contain six, seven, and six zero bytes respectively. |
| 157 | 12 | `component_reference` | `bytes[12]` | little | spec | Marked references at record offsets 146, 157, and 169 name the Canvas scope, owning component entity suffix, and image-asset record; their trailing fields contain six, seven, and six zero bytes respectively. |
| 169 | 11 | `asset_reference` | `bytes[11]` | little | spec | Marked references at record offsets 146, 157, and 169 name the Canvas scope, owning component entity suffix, and image-asset record; their trailing fields contain six, seven, and six zero bytes respectively. |
| 180 | 1 | `second_boundary_present` | `u8` | little | spec | The image-asset reference is followed by `01` at offset 180 and four f64 values at offsets 181, 189, 197, and 205 forming the opposite boundary segment. |
| 181 | 32 | `second_boundary` | `f64[4]` | little | spec | The image-asset reference is followed by `01` at offset 180 and four f64 values at offsets 181, 189, 197, and 205 forming the opposite boundary segment. |
| 213 | 4 | `label_code_unit_count` | `u32` | little | spec | An LP-UTF16 persistent label begins at offset 213 and ends at the paired same-index geometry header. |

## `canvas_geometry_base`

Spec §3.1 · layout: byte offsets · size: 30 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The paired header is followed by eight zero bytes and a marked reference to the same owning component with six trailing zero bytes. |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | The paired header is followed by eight zero bytes and a marked reference to the same owning component with six trailing zero bytes. |
| 19 | 11 | `component_reference` | `bytes[11]` | little | spec | The paired header is followed by eight zero bytes and a marked reference to the same owning component with six trailing zero bytes. |

## `canvas_image_asset_prefix`

Spec §3.1 · layout: byte offsets · size: 25 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A standalone image-asset record follows immediately; its eleven-byte indexed header is followed by ten zero bytes and an LP-UTF16 archive-entry basename ending at the Canvas scope header. |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | A standalone image-asset record follows immediately; its eleven-byte indexed header is followed by ten zero bytes and an LP-UTF16 archive-entry basename ending at the Canvas scope header. |
| 21 | 4 | `name_code_unit_count` | `u32` | little | spec | A standalone image-asset record follows immediately; its eleven-byte indexed header is followed by ten zero bytes and an LP-UTF16 archive-entry basename ending at the Canvas scope header. |

## `canvas_scope_compact_prefix`

Spec §3.1 · layout: byte offsets · size: 26 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The compact prologue has ten zero bytes at offsets 11 through 20 and the marked geometry-record reference at offset 21. |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The compact prologue has ten zero bytes at offsets 11 through 20 and the marked geometry-record reference at offset 21. |
| 21 | 5 | `geometry_reference` | `bytes[5]` | little | spec | The compact prologue has ten zero bytes at offsets 11 through 20 and the marked geometry-record reference at offset 21. |

## `canvas_scope_expanded_prefix`

Spec §3.1 · layout: byte offsets · size: 30 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The expanded prologue has nine zero bytes at scope offsets 11 through 19, a marked null u32 at offset 20, and a marked geometry-record reference at offset 25. |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | The expanded prologue has nine zero bytes at scope offsets 11 through 19, a marked null u32 at offset 20, and a marked geometry-record reference at offset 25. |
| 20 | 5 | `null_reference` | `bytes[5]` | little | spec | The expanded prologue has nine zero bytes at scope offsets 11 through 19, a marked null u32 at offset 20, and a marked geometry-record reference at offset 25. |
| 25 | 5 | `geometry_reference` | `bytes[5]` | little | spec | The expanded prologue has nine zero bytes at scope offsets 11 through 19, a marked null u32 at offset 20, and a marked geometry-record reference at offset 25. |

## `placement_indexed_prefix`

Spec §3.1 · layout: byte offsets · size: 21 B

Class members follow this 21-byte prefix. Version 4 admits marked and unmarked matrix forms.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is |
| 11 | 10 | `zero_prefix` | `bytes[10]` | little | spec | The bytes from indexed-header offset 11 through 20 are zero |

## `surface_patch_boundary_settings_prefix`

Spec §3.1 · layout: byte offsets · size: 38 B

Offsets are relative to the settings record's primary header R. The marked model reference begins at 38 and has a variable GUID carrier.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 19 | `indexed_header` | `bytes[19]` | little | spec | The boundary-settings indexed header is |
| 19 | 2 | `prologue` | `bytes[2]` | little | spec | Its 19 bytes are followed by the two zero prologue bytes |
| 21 | 1 | `is_seed_selection` | `u8` | little | spec | `IsSeedSel` at `R + 21` |
| 22 | 4 | `continuity` | `u32` | little | spec | `PatchContinuity` at `R + 22` |
| 26 | 4 | `side_ordinal` | `u32` | little | spec | `PatchFlip` at indexed-record offset `R + 26` |
| 30 | 8 | `raw_weight` | `f64` | little | spec | `PatchScale` at `R + 30` |

## `surface_patch_v7_normal_flip_prefix`

Spec §3.1 · layout: byte offsets · size: 36 B

Only type version 7 assigns normal reversal to primary-scope offset 31.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 31 | `scope_prefix` | `bytes[31]` | little | spec | In a version-7 `SurfacePatch` type |
| 31 | 1 | `normal_flip` | `u8` | little | spec | the byte at primary-scope offset `P + 31` reverses the whole created surface normal |
| 32 | 4 | `following_word` | `u32` | little | spec | The following `u32le` at `P + 32` is `4` · value `4` |

## `hem_common_controls`

Spec §3.1 · layout: byte offsets · size: 41 B

Offsets are relative to C = 85 + S, shared by gap-and-length, rolled and teardrop Hem frames.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `bend_position` | `u32` | little | spec | code `3` selects Adjacent |
| 30 | 4 | `compatibility_one` | `u32` | little | spec | The u32 at `C+30` is the compatibility value `1` |
| 34 | 2 | `legacy_references` | `bytes[2]` | little | spec | `C+34` and `C+35` are null optional legacy references |
| 36 | 4 | `compatibility_four` | `u32` | little | spec | the u32 at `C+36` is the compatibility value `4` |
| 40 | 1 | `flip` | `u8` | little | spec | The Flip byte at `C+40` |

Unstated regions:

- `4..30` (26 B): Edge count and the edge-wrapper and settings references occupy these bytes.

## `sheet_metal_settings_prefix`

Spec §3.1 · layout: byte offsets · size: 14 B

Offsets are relative to the end of the variable-length primary indexed header. Versioned optional references follow this prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 10 | 4 | `override_mask` | `u32` | little | spec | ten zero bytes and a u32 override mask |

Unstated regions:

- `0..10` (10 B): Ten zero bytes precede the override mask.

## `sheet_metal_bend_radius_override_prefix`

Spec §3.1 · layout: byte offsets · size: 21 B

Offsets are relative to the end of the indexed header of the BendRadius override bundle.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 10 | 11 | `bend_radius_owner` | `bytes[11]` | little | spec | precede a reference to the parameter owner |

Unstated regions:

- `0..10` (10 B): Ten zero bytes follow the indexed header.

## `sheet_metal_relief_override_prefix`

Spec §3.1 · layout: byte offsets · size: 47 B

Offsets are relative to the end of the indexed header of the relief override bundle. The width owner follows the counted depth and remnant references.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 10 | 4 | `count` | `u32` | little | spec | precede u32 count `2` · value `2` |
| 14 | 11 | `bend_depth_owner` | `bytes[11]` | little | spec | owner references for BendDepth and BendRemnant in that order |
| 25 | 11 | `bend_remnant_owner` | `bytes[11]` | little | spec | owner references for BendDepth and BendRemnant in that order |
| 36 | 11 | `bend_width_owner` | `bytes[11]` | little | spec | then an owner reference for BendWidth |

Unstated regions:

- `0..10` (10 B): Ten zero bytes follow the indexed header.

## `local_component_occurrence_prefix`

Spec §3.1 · layout: byte offsets · size: 209 B

Offsets are relative to the carrier's indexed header. The identity tail or the matrix tail follows at offset 209.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 8 | `empty_instance_name` | `bytes[8]` | little | spec | the frame stores u32 `0` and an empty LP-ASCII instance name |
| 19 | 1 | `path_marker` | `u8` | little | spec | `u8 1` at offset 19 · value `1` |
| 20 | 4 | `path_count` | `u32` | little | spec | u32 path count `1` at offset 20 · value `1` |
| 24 | 9 | `component_reference` | `bytes[9]` | little | spec | Marked component-record references at offsets 24 and 197 |
| 35 | 4 | `path_discriminator` | `u32` | little | spec | The u32 instance discriminator is at offset 35 |
| 39 | 1 | `path_terminator` | `u8` | little | spec | the path terminator is at offset 39 · value `0` |
| 40 | 4 | `occurrence_ordinal` | `u32` | little | spec | the one-based occurrence ordinal at offset 40 |
| 44 | 76 | `carrier_guid` | `bytes[76]` | little | spec | an LP-UTF16 carrier GUID at offset 44 |
| 120 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | an LP-UTF16 occurrence GUID at offset 120 |
| 197 | 9 | `component_reference_repeat` | `bytes[9]` | little | spec | Marked component-record references at offsets 24 and 197 |
| 208 | 1 | `identity_flag` | `u8` | little | spec | Offset 208 is the identity flag. |

Unstated regions:

- `0..11` (11 B): Indexed header.
- `33..35` (2 B): Two zero trailing bytes of the local component reference.
- `196..197` (1 B): Zero separator.
- `206..208` (2 B): Zero trailing field of the repeated component-record reference.

## `local_component_occurrence_identity_tail`

Spec §3.1 · layout: byte offsets · size: 20 B

Offsets are relative to the end of the 209-byte prefix; the identity frame is 229 bytes.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 9 | 1 | `owner_marker` | `u8` | little | spec | Nine zero bytes and the marker `u8 1` follow the flag or the matrix. · value `1` |
| 10 | 8 | `owner_entity` | `u64` | little | spec | The u64 owner entity is at offset 219 of the identity frame |

Unstated regions:

- `0..9` (9 B): Nine zero bytes.
- `18..20` (2 B): Two zero bytes close the frame.

## `local_component_occurrence_matrix_tail`

Spec §3.1 · layout: byte offsets · size: 148 B

Offsets are relative to the end of the 209-byte prefix; the matrix frame is 357 bytes.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 128 | `transform` | `f64[16]` | little | spec | a row-major rigid local-to-container transform at offset 209 |
| 137 | 1 | `owner_marker` | `u8` | little | spec | Nine zero bytes and the marker `u8 1` follow the flag or the matrix. · value `1` |
| 138 | 8 | `owner_entity` | `u64` | little | spec | offset 347 of the matrix frame |

Unstated regions:

- `128..137` (9 B): Nine zero bytes.
- `146..148` (2 B): Two zero bytes close the frame.

## `root_instance_prefix`

Spec §3.1 · layout: byte offsets · size: 45 B

Offsets are relative to the root-instance record's indexed header. The record continues after the component-definition reference.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 4 | `leading_zero` | `u32` | little | spec | After its indexed header that record stores u32 `0` · value `0` |
| 15 | 4 | `instance_name_length` | `u32` | little | spec | LP-ASCII `rootInstance` · value `12` |
| 19 | 12 | `instance_name` | `bytes[12]` | little | spec | LP-ASCII `rootInstance` |
| 31 | 1 | `path_marker` | `u8` | little | spec | `u8 1`, u32 path count `1` · value `1` |
| 32 | 4 | `path_count` | `u32` | little | spec | `u8 1`, u32 path count `1` · value `1` |
| 36 | 1 | `component_marker` | `u8` | little | spec | a marked u64 component-definition reference · value `1` |
| 37 | 8 | `component_reference` | `u64` | little | spec | a marked u64 component-definition reference |

Unstated regions:

- `0..11` (11 B): Indexed header.

## `assembly_wide_operand_path_locator`

Spec §Assembly operands · layout: byte offsets · size: not stated

This size-less byte record describes the fixed prefix through the optional transform. The scope backlink, wrapper reference, constant 2 and zero tail follow one zero byte after the identity flag or the transform.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte header is followed by 28 zero bytes |
| 11 | 28 | `zero_run_28` | `bytes[28]` | little | spec | header is followed by 28 zero bytes |
| 39 | 4 | `constant_three` | `u32` | little | spec | u32 value 3 at offset 39 |
| 43 | 13 | `zero_run_13` | `bytes[13]` | little | spec | 13 zero bytes |
| 56 | 11 | `nonzero_record_reference` | `bytes[11]` | little | spec | a nonzero marked same-segment reference at offset 56 |
| 67 | 1 | `identity_flag` | `u8` | little | spec | an identity flag byte at offset 67 |
| 68 | 128 | `transform` | `f64[16]` | little | spec | a row-major rigid 4×4 transform at offset 68 |

## `canvas_scope_tail_98`

Spec §3.1 · layout: byte offsets · size: 98 B

Offsets are relative to the end of the LP-UTF16 Canvas kind.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `feature_ordinal` | `u32` | little | spec | a positive feature ordinal |
| 4 | 8 | `zero_prefix` | `bytes[8]` | little | spec | eight zero bytes |
| 12 | 4 | `member_count` | `u32` | little | spec | member count `1` · value `1` |
| 16 | 11 | `self_reference` | `bytes[11]` | little | spec | a marked self reference |
| 27 | 3 | `zero_before_companion` | `bytes[3]` | little | spec | three zero bytes |
| 30 | 11 | `first_companion` | `bytes[11]` | little | spec | a marked companion reference |
| 41 | 1 | `zero_before_history` | `u8` | little | spec | a zero separator · value `0` |
| 42 | 4 | `history_state` | `u32` | little | spec | null history-state identity `0xffffffff` · value `4294967295` |
| 46 | 4 | `epoch` | `u32` | little | spec | evaluation epoch |
| 50 | 8 | `duration` | `f64` | little | spec | finite nonnegative evaluation duration |
| 58 | 4 | `repeated_epoch` | `u32` | little | spec | the repeated evaluation epoch |
| 62 | 11 | `second_companion` | `bytes[11]` | little | spec | a second marked companion reference |
| 73 | 2 | `zero_before_third` | `bytes[2]` | little | spec | two zero bytes |
| 75 | 11 | `third_companion` | `bytes[11]` | little | spec | a third marked companion reference |
| 86 | 11 | `closing_self_reference` | `bytes[11]` | little | spec | a closing marked self reference |
| 97 | 1 | `zero_tail` | `u8` | little | spec | and a zero byte · value `0` |

## `canvas_geometry_legacy_prefix`

Spec §3.1 · layout: byte offsets · size: 214 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 11 | 13 | `geometry_prologue` | `bytes[13]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 24 | 32 | `first_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 56 | 11 | `plane_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 67 | 77 | `geometry_payload` | `bytes[77]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 144 | 11 | `scope_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 155 | 11 | `component_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 166 | 11 | `asset_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 177 | 1 | `second_boundary_present` | `u8` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 178 | 32 | `second_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 210 | 4 | `label_code_unit_count` | `u32` | little | spec | Canvas geometry has six fixed prefix layouts. |

## `canvas_geometry_transitional_prefix`

Spec §3.1 · layout: byte offsets · size: 216 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 11 | 14 | `geometry_prologue` | `bytes[14]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 25 | 32 | `first_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 57 | 11 | `plane_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 68 | 77 | `geometry_payload` | `bytes[77]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 145 | 11 | `scope_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 156 | 12 | `component_reference` | `bytes[12]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 168 | 11 | `asset_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 179 | 1 | `second_boundary_present` | `u8` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 180 | 32 | `second_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 212 | 4 | `label_code_unit_count` | `u32` | little | spec | Canvas geometry has six fixed prefix layouts. |

## `canvas_geometry_extra12_prefix`

Spec §3.1 · layout: byte offsets · size: 229 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 11 | 15 | `geometry_prologue` | `bytes[15]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 26 | 32 | `first_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 58 | 11 | `plane_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 69 | 12 | `plane_extension` | `u32[3]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 81 | 77 | `geometry_payload` | `bytes[77]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 158 | 11 | `scope_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 169 | 12 | `component_reference` | `bytes[12]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 181 | 11 | `asset_reference` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 192 | 1 | `second_boundary_present` | `u8` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 193 | 32 | `second_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 225 | 4 | `label_code_unit_count` | `u32` | little | spec | Canvas geometry has six fixed prefix layouts. |

## `canvas_geometry_typed_legacy_prefix`

Spec §3.1 · layout: byte offsets · size: 374 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 11 | 13 | `geometry_prologue` | `bytes[13]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 24 | 32 | `first_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 56 | 51 | `plane_reference` | `bytes[51]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 107 | 77 | `geometry_payload` | `bytes[77]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 184 | 51 | `scope_reference` | `bytes[51]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 235 | 51 | `component_reference` | `bytes[51]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 286 | 51 | `asset_reference` | `bytes[51]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 337 | 1 | `second_boundary_present` | `u8` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 338 | 32 | `second_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 370 | 4 | `label_code_unit_count` | `u32` | little | spec | Canvas geometry has six fixed prefix layouts. |

## `canvas_geometry_typed_transitional_prefix`

Spec §3.1 · layout: byte offsets · size: 376 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 11 | 14 | `geometry_prologue` | `bytes[14]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 25 | 32 | `first_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 57 | 51 | `plane_reference` | `bytes[51]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 108 | 77 | `geometry_payload` | `bytes[77]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 185 | 51 | `scope_reference` | `bytes[51]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 236 | 52 | `component_reference` | `bytes[52]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 288 | 51 | `asset_reference` | `bytes[51]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 339 | 1 | `second_boundary_present` | `u8` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 340 | 32 | `second_boundary` | `f64[4]` | little | spec | Canvas geometry has six fixed prefix layouts. |
| 372 | 4 | `label_code_unit_count` | `u32` | little | spec | Canvas geometry has six fixed prefix layouts. |

## `canvas_scope_typed_tail`

Spec §3.1 · layout: byte offsets · size: 196 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `feature_ordinal` | `u32` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 4 | 15 | `zero_prefix` | `bytes[15]` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 19 | 51 | `first_companion` | `bytes[51]` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 70 | 4 | `history_state` | `u32` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 74 | 4 | `epoch` | `u32` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 78 | 8 | `duration` | `f64` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 86 | 4 | `repeated_epoch` | `u32` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 90 | 51 | `second_companion` | `bytes[51]` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 141 | 2 | `zero_before_third` | `bytes[2]` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 143 | 51 | `third_companion` | `bytes[51]` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |
| 194 | 2 | `zero_tail` | `bytes[2]` | little | spec | A typed Canvas scope has a 196-byte fixed tail. |

## `thread_bound_hole_tail`

Spec §3.1 · layout: byte offsets · size: 109 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `feature_ordinal` | `u32` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 4 | 8 | `zero_prefix` | `bytes[8]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 12 | 4 | `member_count` | `u32` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 16 | 11 | `self_reference` | `bytes[11]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 27 | 11 | `thread_reference` | `bytes[11]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 38 | 3 | `zero_before_companion` | `bytes[3]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 41 | 11 | `first_companion` | `bytes[11]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 52 | 1 | `zero_before_history` | `u8` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 53 | 4 | `previous_history_state` | `u32` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 57 | 4 | `epoch` | `u32` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 61 | 8 | `duration` | `f64` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 69 | 4 | `repeated_epoch` | `u32` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 73 | 11 | `second_companion` | `bytes[11]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 84 | 2 | `zero_before_third` | `bytes[2]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 86 | 11 | `third_companion` | `bytes[11]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 97 | 11 | `closing_self_reference` | `bytes[11]` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |
| 108 | 1 | `zero_tail` | `u8` | little | spec | A Thread-bound Hole scope has a 109-byte reciprocal tail. |

## `assembly_compact_wide_scope`

Spec §Assembly operands · layout: byte offsets · size: not stated

This size-less byte record describes the fixed operand prefix of the 806-, 850- and 861-byte scopes. Offsets are relative to the scope's primary indexed header; an LP-UTF16 GUID follows each transform.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_frames.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The 806-, 850- and 861-byte `Assemble` scopes use the compact wide |
| 11 | 10 | `zero_run_11` | `bytes[10]` | little | spec | Bytes 11 through 20 are zero |
| 21 | 1 | `operand_flag` | `u8` | little | spec | byte 21 is the frame-sense byte |
| 22 | 10 | `zero_run_22` | `bytes[10]` | little | spec | bytes 22 through 31 are zero |
| 32 | 11 | `first_operand_reference` | `bytes[11]` | little | spec | the operand references are at offsets 32 and 254 |
| 43 | 7 | `first_reference_zero_run` | `bytes[7]` | little | spec | seven zero bytes follow each reference |
| 50 | 128 | `first_operand_transform` | `f64[16]` | little | spec | row-major transforms start at offsets 50 and 272 |
| 178 | 76 | `first_operand_guid` | `bytes[76]` | little | spec | an LP-UTF16 GUID follows each transform |
| 254 | 11 | `second_operand_reference` | `bytes[11]` | little | spec | the operand references are at offsets 32 and 254 |
| 265 | 7 | `second_reference_zero_run` | `bytes[7]` | little | spec | seven zero bytes follow each reference |
| 272 | 128 | `second_operand_transform` | `f64[16]` | little | spec | row-major transforms start at offsets 50 and 272 |

## `body_reference_tail_prefix`

Spec §3.1 · layout: byte offsets · size: 25 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `header` | `bytes[11]` | little | spec | A class-365 whole-body member starts after its indexed header with ten zero bytes |
| 11 | 10 | `zero_prefix` | `bytes[10]` | little | spec | A class-365 whole-body member starts after its indexed header with ten zero bytes |
| 21 | 4 | `reference_count` | `u32` | little | spec | A class-365 whole-body member starts after its indexed header with ten zero bytes |

## `body_reference_persistent_pair`

Spec §3.1 · layout: byte offsets · size: 12 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `reference` | `u64` | little | spec | ordered `(u64 persistent Design reference, u32 form)` pairs |
| 8 | 4 | `form` | `u32` | little | spec | ordered `(u64 persistent Design reference, u32 form)` pairs |

## `base_feature_local_reference_field`

Spec §3.1 · layout: byte offsets · size: 6 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `cross_document` | `u8` | little | spec | cross_document:u8 0 · value `0` |
| 1 | 1 | `different_segment` | `u8` | little | spec | different_segment:u8 0 · value `0` |
| 2 | 4 | `member` | `u32` | little | spec | member:u32 |

## `construction_operand_affine_transform_frame`

Spec §3.1 · layout: byte offsets · size: 152 B

Offsets are relative to the transform's indexed header. The following active indexed record begins at offset 152 and retains an independent record index.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/operands.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A construction-operand affine transform is a 152-byte indexed record |
| 22 | 128 | `matrix` | `bytes[128]` | little | spec | sixteen row-major f64 values |
| 150 | 2 | `trailer` | `bytes[2]` | little | spec | and bytes `01 00` |

Unstated regions:

- `11..22` (11 B): Eleven zero bytes follow the indexed header.

## `construction_operand_compact_relative_placement`

Spec §3.1 · layout: byte offsets · size: 24 B

Offsets are relative to the indexed header. The boolean is retained independently of the fixed compact marker.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A compact relative-placement carrier is a 24-byte indexed record |
| 21 | 1 | `compact_marker` | `u8` | little | spec | byte `01`, one boolean, and byte `00` · value `1` |
| 22 | 1 | `value` | `u8` | little | spec | byte `01`, one boolean, and byte `00` |
| 23 | 1 | `trailer` | `u8` | little | spec | byte `01`, one boolean, and byte `00` · value `0` |

Unstated regions:

- `11..21` (10 B): Ten zero bytes follow the indexed header.

## `legacy_occurrence_target_reference`

Spec §1.4 · layout: byte offsets · size: 60 B

Offsets are relative to the reference presence marker. The repeated references carry the same entity, source occurrence identity and type GUID.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `presence` | `u8` | little | spec | Each legacy target reference is · value `1` |
| 1 | 8 | `target_entity` | `u64` | little | spec | Each legacy target reference is |
| 9 | 1 | `identity_presence` | `u8` | little | spec | Each legacy target reference is · value `1` |
| 10 | 8 | `source_occurrence_identity` | `u64` | little | spec | Each legacy target reference is |
| 18 | 1 | `type_marker` | `u8` | little | spec | Each legacy target reference is · value `0` |
| 19 | 4 | `type_guid_length` | `u32` | little | spec | Each legacy target reference is · value `36` |
| 23 | 36 | `type_guid` | `bytes[36]` | little | spec | Each legacy target reference is |
| 59 | 1 | `terminator` | `u8` | little | spec | Each legacy target reference is · value `0` |

## `local_component_occurrence_attributed_prefix`

Spec §3.1 · layout: byte offsets · size: 44 B

Offsets are relative to the indexed header. The typed attribute run begins immediately after this prefix. The path discriminator is independent of the later occurrence ordinal.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An attributed local component occurrence carries |
| 11 | 8 | `empty_instance_name` | `bytes[8]` | little | spec | An attributed local component occurrence carries |
| 19 | 1 | `path_marker` | `u8` | little | spec | An attributed local component occurrence carries · value `1` |
| 20 | 4 | `path_count` | `u32` | little | spec | An attributed local component occurrence carries · value `1` |
| 24 | 9 | `component_reference` | `bytes[9]` | little | spec | An attributed local component occurrence carries |
| 35 | 4 | `path_discriminator` | `u32` | little | spec | An attributed local component occurrence carries |
| 39 | 1 | `attributes_marker` | `u8` | little | spec | An attributed local component occurrence carries · value `1` |
| 40 | 4 | `attribute_count` | `u32` | little | spec | An attributed local component occurrence carries |

Unstated regions:

- `33..35` (2 B): Two zero trailing bytes of the local component reference.

## `sketch_point_legacy_tail`

Spec §3.1 · layout: byte offsets · size: 93 B

Relative to the first flag byte; excludes the preceding property and first paired reference.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `flag` | `u8` | little | spec | The legacy tail contains the flag |
| 1 | 16 | `coordinates` | `f64[2]` | little | spec | coordinates |
| 17 | 20 | `coordinate_zero_lane` | `bytes[20]` | little | spec | zero lanes |
| 37 | 4 | `unit_scalar` | `f32` | little | spec | f32 constants |
| 41 | 12 | `zero_lane` | `bytes[12]` | little | spec | zero lane |
| 53 | 8 | `unit_pair` | `f32[2]` | little | spec | f32 constants |
| 61 | 10 | `marker` | `bytes[10]` | little | spec | marker |
| 71 | 11 | `paired_reference` | `bytes[11]` | little | spec | repeated paired reference |
| 82 | 11 | `owner_reference` | `bytes[11]` | little | spec | owner reference |

## `sketch_point_v8_tail_prefix`

Spec §3.1 · layout: byte offsets · size: 61 B

Relative to the first flag byte; excludes the preceding property and first paired reference.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 7 | `flags` | `bytes[7]` | little | spec | Each modern fixed prefix contains the flags |
| 7 | 24 | `coordinates` | `f64[3]` | little | spec | coordinates |
| 31 | 8 | `selector` | `u64` | little | spec | selector |
| 39 | 1 | `state` | `u8` | little | spec | state |
| 40 | 8 | `zero_lane` | `bytes[8]` | little | spec | zero lane |
| 48 | 8 | `unit_pair` | `f32[2]` | little | spec | f32 constants |
| 56 | 5 | `marker` | `bytes[5]` | little | spec | marker |

## `sketch_point_v10_tail_prefix`

Spec §3.1 · layout: byte offsets · size: 65 B

Relative to the first flag byte; excludes the preceding property and first paired reference.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 7 | `flags` | `bytes[7]` | little | spec | Each modern fixed prefix contains the flags |
| 7 | 24 | `coordinates` | `f64[3]` | little | spec | coordinates |
| 31 | 8 | `selector` | `u64` | little | spec | selector |
| 39 | 1 | `state` | `u8` | little | spec | state |
| 40 | 12 | `zero_lane` | `bytes[12]` | little | spec | zero lane |
| 52 | 8 | `unit_pair` | `f32[2]` | little | spec | f32 constants |
| 60 | 5 | `marker` | `bytes[5]` | little | spec | marker |

## `sketch_point_v11_tail_prefix`

Spec §3.1 · layout: byte offsets · size: 66 B

Relative to the first flag byte; excludes the preceding property and first paired reference.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `flags` | `bytes[8]` | little | spec | Each modern fixed prefix contains the flags |
| 8 | 24 | `coordinates` | `f64[3]` | little | spec | coordinates |
| 32 | 8 | `selector` | `u64` | little | spec | selector |
| 40 | 1 | `state` | `u8` | little | spec | state |
| 41 | 12 | `zero_lane` | `bytes[12]` | little | spec | zero lane |
| 53 | 8 | `unit_pair` | `f32[2]` | little | spec | f32 constants |
| 61 | 5 | `marker` | `bytes[5]` | little | spec | marker |

## `sketch_curve_tail_1_reference`

Spec §3.1 · layout: byte offsets · size: 25 B

N = 1. Offsets are relative to the end of the 96-byte analytic payload.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/sketch_project.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `reference_marker` | `u8` | little | spec | `0x01` |
| 1 | 4 | `reference_record_index` | `u32` | little | spec | u32 `record_index` |
| 5 | 6 | `reference_zero_trailer` | `bytes[6]` | little | spec | six zero bytes |
| 11 | 5 | `zero_pad` | `bytes[5]` | little | spec | `P = 5` when `N = 1` |
| 16 | 4 | `opaque` | `bytes[4]` | little | spec | four opaque bytes |
| 20 | 4 | `flag_guard` | `u32` | little | spec | u32 `1` |
| 24 | 1 | `construction_flag` | `u8` | little | spec | u8 construction flag |

## `sketch_curve_tail_2_references`

Spec §3.1 · layout: byte offsets · size: 34 B

N = 2. Offsets are relative to the end of the 96-byte analytic payload.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/sketch_project.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `first_reference_marker` | `u8` | little | spec | `0x01` |
| 1 | 4 | `first_reference_record_index` | `u32` | little | spec | u32 `record_index` |
| 5 | 6 | `first_reference_zero_trailer` | `bytes[6]` | little | spec | six zero bytes |
| 11 | 1 | `second_reference_marker` | `u8` | little | spec | `0x01` |
| 12 | 4 | `second_reference_record_index` | `u32` | little | spec | u32 `record_index` |
| 16 | 6 | `second_reference_zero_trailer` | `bytes[6]` | little | spec | six zero bytes |
| 22 | 3 | `zero_pad` | `bytes[3]` | little | spec | `P = 3` when `N` is `2` or `3` |
| 25 | 4 | `opaque` | `bytes[4]` | little | spec | four opaque bytes |
| 29 | 4 | `flag_guard` | `u32` | little | spec | u32 `1` |
| 33 | 1 | `construction_flag` | `u8` | little | spec | u8 construction flag |

## `sketch_curve_tail_3_references`

Spec §3.1 · layout: byte offsets · size: 45 B

N = 3. Offsets are relative to the end of the 96-byte analytic payload.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/sketch_project.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `first_reference_marker` | `u8` | little | spec | `0x01` |
| 1 | 4 | `first_reference_record_index` | `u32` | little | spec | u32 `record_index` |
| 5 | 6 | `first_reference_zero_trailer` | `bytes[6]` | little | spec | six zero bytes |
| 11 | 1 | `second_reference_marker` | `u8` | little | spec | `0x01` |
| 12 | 4 | `second_reference_record_index` | `u32` | little | spec | u32 `record_index` |
| 16 | 6 | `second_reference_zero_trailer` | `bytes[6]` | little | spec | six zero bytes |
| 22 | 1 | `third_reference_marker` | `u8` | little | spec | `0x01` |
| 23 | 4 | `third_reference_record_index` | `u32` | little | spec | u32 `record_index` |
| 27 | 6 | `third_reference_zero_trailer` | `bytes[6]` | little | spec | six zero bytes |
| 33 | 3 | `zero_pad` | `bytes[3]` | little | spec | `P = 3` when `N` is `2` or `3` |
| 36 | 4 | `opaque` | `bytes[4]` | little | spec | four opaque bytes |
| 40 | 4 | `flag_guard` | `u32` | little | spec | u32 `1` |
| 44 | 1 | `construction_flag` | `u8` | little | spec | u8 construction flag |

## `sheet_metal_region_carrier_prefix`

Spec §3.1 · layout: byte offsets · size: 36 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | indexed header |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | ten zero bytes |
| 21 | 1 | `owner_marker` | `u8` | little | spec | marked owning-scope reference |
| 22 | 4 | `owner_record_index` | `u32` | little | spec | marked owning-scope reference |
| 26 | 6 | `owner_zero_trailer` | `bytes[6]` | little | spec | six zero trailer bytes |
| 32 | 4 | `identity_count` | `u32` | little | spec | nonzero u32 identity count |

## `sheet_metal_region_identity_reference`

Spec §3.1 · layout: byte offsets · size: 11 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | marker byte `1` |
| 1 | 4 | `record_index` | `u32` | little | spec | u32 record index |
| 5 | 6 | `zero_trailer` | `bytes[6]` | little | spec | six zero bytes |

## `sheet_metal_region_carrier_scalar_lane`

Spec §3.1 · layout: byte offsets · size: 16 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `discriminator` | `u32` | little | spec | A u32 discriminator |
| 4 | 8 | `scalar` | `f64` | little | spec | a finite f64 scalar |
| 12 | 4 | `repeated_discriminator` | `u32` | little | spec | an equal repeated u32 discriminator |

## `compact_owner_body_wrapper`

Spec §3.1 · layout: byte offsets · size: 33 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `header` | `bytes[11]` | little | spec | The compact owner-body wrapper has EntityTracking type GUID |
| 11 | 10 | `zero_prefix` | `bytes[10]` | little | spec | The compact owner-body wrapper has EntityTracking type GUID |
| 21 | 1 | `reference_marker` | `u8` | little | spec | The compact owner-body wrapper has EntityTracking type GUID |
| 22 | 8 | `carrier_record_index` | `u64` | little | spec | The compact owner-body wrapper has EntityTracking type GUID |
| 30 | 3 | `zero_suffix` | `bytes[3]` | little | spec | The compact owner-body wrapper has EntityTracking type GUID |

## `compact_owner_body_carrier`

Spec §3.1 · layout: byte offsets · size: 29 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `header` | `bytes[11]` | little | spec | The adjacent carrier has EntityTracking type GUID |
| 11 | 10 | `zero_prefix` | `bytes[10]` | little | spec | The adjacent carrier has EntityTracking type GUID |
| 21 | 8 | `body_suffix` | `u64` | little | spec | The adjacent carrier has EntityTracking type GUID |

## `paramesh_flagged_mesh_collection_prefix`

Spec §3.1 · layout: byte offsets · size: 42 B

Offsets are relative to the mesh-collection indexed header. The nested CommonData record starts at the end of this prefix.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by ten zero bytes |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | in place of the last of the ten zero bytes |
| 20 | 1 | `flag` | `u8` | little | spec | store byte `01` at offset 20 · value `1` |
| 21 | 4 | `body_count` | `u32` | little | spec | a u32 body count at offset 21 |
| 25 | 4 | `flagged_word` | `u32` | little | spec | a u32 at offset 25 after its first body count |
| 29 | 2 | `constant_01_01` | `bytes[2]` | little | spec | Its bytes `01 01`, texture-table reference, and nested record then start at offsets 29, 31, and 42 |
| 31 | 11 | `texture_table_reference` | `bytes[11]` | little | spec | Its bytes `01 01`, texture-table reference, and nested record then start at offsets 29, 31, and 42 |

## `referenced_angle_revolve_prologue`

Spec §3.1 · layout: byte offsets · size: 50 B

Offsets are relative to the Revolve primary indexed header. The marked angle-owner reference occupies offsets 39 through 49.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/path_feature.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 1 | `marker` | `u8` | little | spec | It uses the marker-one prologue through offset 33 |
| 21 | 4 | `zero_value` | `u32` | little | spec | It uses the marker-one prologue through offset 33 |
| 25 | 4 | `operation` | `u32` | little | spec | It uses the marker-one prologue through offset 33 |
| 29 | 4 | `extent_kind` | `u32` | little | spec | It uses the marker-one prologue through offset 33 |
| 33 | 1 | `direction_kind` | `u8` | little | spec | It uses the marker-one prologue through offset 33 |
| 34 | 1 | `project_axis` | `u8` | little | spec | Offset 34 stores the Project Axis flag |
| 35 | 4 | `structural_constant` | `u32` | little | spec | Offset 35 stores u32 one |
| 39 | 1 | `angle_reference_marker` | `u8` | little | spec | The marked angle-owner reference starts at offset 39 |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding Revolve envelope are outside this field run.
- `40..50` (10 B): The marked reference stores the u32 angle-owner record index followed by six zero bytes.

## `two_sided_revolve_prologue`

Spec §3.1 · layout: byte offsets · size: 60 B

Offsets are relative to the Revolve primary indexed header. The marked opposite-angle reference occupies offsets 38 through 48 and the marked angle-owner reference occupies offsets 49 through 59.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/path_feature.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 1 | `marker` | `u8` | little | spec | It uses the marker-one prologue through offset 33, and offset 34 |
| 21 | 4 | `zero_value` | `u32` | little | spec | It uses the marker-one prologue through offset 33, and offset 34 |
| 25 | 4 | `operation` | `u32` | little | spec | It uses the marker-one prologue through offset 33, and offset 34 |
| 29 | 4 | `extent_kind` | `u32` | little | spec | It uses the marker-one prologue through offset 33, and offset 34 |
| 33 | 1 | `direction_kind` | `u8` | little | spec | It uses the marker-one prologue through offset 33, and offset 34 |
| 34 | 1 | `project_axis` | `u8` | little | spec | and offset 34 stores the Project Axis flag |
| 35 | 1 | `solid` | `u8` | little | spec | Offset 35 stores the solid flag `1` |
| 36 | 1 | `symmetric` | `u8` | little | spec | Offset 36 stores the Symmetric flag |
| 37 | 1 | `two_sides` | `u8` | little | spec | offset 37 stores the Two Sides flag |
| 38 | 1 | `opposite_angle_reference_marker` | `u8` | little | spec | The marked opposite-angle reference starts at offset 38 |
| 49 | 1 | `angle_reference_marker` | `u8` | little | spec | The marked angle-owner reference starts at offset 49 |

Unstated regions:

- `0..20` (20 B): The indexed header and the preceding Revolve envelope are outside this field run.
- `39..49` (10 B): The marked reference stores the u32 opposite-angle owner record index followed by six zero bytes.
- `50..60` (10 B): The marked reference stores the u32 angle-owner record index followed by six zero bytes.

## `typed_reference_parameter_scope_tail`

Spec §3.1 · layout: byte offsets · size: 196 B

Offsets are relative to the u32 feature ordinal that begins the fixed scope tail after the LP-UTF16 kind.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/parameter_scope.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `ordinal` | `u32` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 4 | 15 | `zero_run_15` | `bytes[15]` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 19 | 51 | `first_reference` | `bytes[51]` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 70 | 4 | `previous_history_state_id` | `u32` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 74 | 4 | `lane` | `u32` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 78 | 8 | `scalar` | `f64` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 86 | 4 | `repeated_lane` | `u32` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 90 | 51 | `second_reference` | `bytes[51]` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 141 | 2 | `second_zero_run_2` | `bytes[2]` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 143 | 51 | `third_reference` | `bytes[51]` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |
| 194 | 2 | `third_zero_run_2` | `bytes[2]` | little | spec | A typed-reference parameter scope, written by the container generation that writes typed references, has a 196-byte form |

## `typed_reference_extrude_prologue`

Spec §3.1 · layout: byte offsets · size: 26 B

Offsets are relative to the Extrude primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/extrude.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 11 | `zero_run_11` | `bytes[11]` | little | spec | A typed-reference Extrude scope stores eleven zero bytes after its indexed header |
| 22 | 4 | `operation` | `u32` | little | spec | A typed-reference Extrude scope stores eleven zero bytes after its indexed header |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `shifted_typed_reference_extrude_prologue`

Spec §3.1 · layout: byte offsets · size: 30 B

Offsets are relative to the Extrude primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/extrude.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | The shifted typed-reference form instead stores nine zero bytes, u32 `1` at offset 20 |
| 20 | 4 | `marker` | `u32` | little | spec | The shifted typed-reference form instead stores nine zero bytes, u32 `1` at offset 20 |
| 24 | 2 | `zero_run_2` | `bytes[2]` | little | spec | The shifted typed-reference form instead stores nine zero bytes, u32 `1` at offset 20 |
| 26 | 4 | `operation` | `u32` | little | spec | The shifted typed-reference form instead stores nine zero bytes, u32 `1` at offset 20 |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `typed_reference_extrude_selection_group_prefix`

Spec §3.1 · layout: byte offsets · size: 76 B

Offsets are relative to the selection-group primary indexed header; 51-byte typed member references follow the count.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/operands.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | A typed-reference Extrude selection group stores ten zero bytes after its indexed header |
| 21 | 51 | `scope_reference` | `bytes[51]` | little | spec | A typed-reference Extrude selection group stores ten zero bytes after its indexed header |
| 72 | 4 | `member_count` | `u32` | little | spec | A typed-reference Extrude selection group stores ten zero bytes after its indexed header |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `typed_reference_profile_selection_prefix`

Spec §3.1 · layout: byte offsets · size: 76 B

Offsets are relative to the selection primary indexed header; the LP-UTF16 asset GUID and owner text follow.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/operands.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | A typed-reference profile selection is a construction-operand group member |
| 21 | 51 | `identity_reference` | `bytes[51]` | little | spec | A typed-reference profile selection is a construction-operand group member |
| 72 | 4 | `constant_one` | `u32` | little | spec | A typed-reference profile selection is a construction-operand group member |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `typed_reference_profile_chain_record`

Spec §3.1 · layout: byte offsets · size: 82 B

Offsets are relative to the chain record indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/operands.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 14 | `zero_run_14` | `bytes[14]` | little | spec | The chain record at the selection's record index plus one stores fourteen zero bytes |
| 25 | 6 | `chain_marker` | `bytes[6]` | little | spec | The chain record at the selection's record index plus one stores fourteen zero bytes |
| 31 | 51 | `sketch_scope_reference` | `bytes[51]` | little | spec | The chain record at the selection's record index plus one stores fourteen zero bytes |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `typed_reference_profile_identity_record`

Spec §3.1 · layout: byte offsets · size: 76 B

Offsets are relative to the identity record indexed header; the two counted region lists begin at offset 72.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/operands.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The identity record at the record index plus three stores ten zero bytes |
| 21 | 51 | `selection_reference` | `bytes[51]` | little | spec | The identity record at the record index plus three stores ten zero bytes |
| 72 | 4 | `first_region_count` | `u32` | little | spec | The identity record at the record index plus three stores ten zero bytes |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `typed_reference_companion_prefix`

Spec §3.1 · layout: byte offsets · size: 98 B

Offsets are relative to the companion indexed header; the companion-owned payload follows the prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 20 | `zero_run_20` | `bytes[20]` | little | spec | A typed-reference companion prefix, written by the container generation |
| 31 | 51 | `owner_reference` | `bytes[51]` | little | spec | A typed-reference companion prefix, written by the container generation |
| 82 | 8 | `timestamp_micros` | `u64` | little | spec | A typed-reference companion prefix, written by the container generation |
| 90 | 8 | `zero_run_8` | `bytes[8]` | little | spec | A typed-reference companion prefix, written by the container generation |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `base_feature_typed_reference_zero_body`

Spec §3.1 · layout: byte offsets · size: 83 B

Offsets are relative to the Base Feature primary indexed header; zero bytes follow the reference up to the null GUID length field.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/base_feature.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 9 | `zero_run_9` | `bytes[9]` | little | spec | The typed-reference zero-body form stores the same prefix with a 51-byte typed shared-metadata reference at offset 32 |
| 20 | 1 | `zero_body_marker` | `u8` | little | spec | The typed-reference zero-body form stores the same prefix with a 51-byte typed shared-metadata reference at offset 32 |
| 21 | 11 | `zero_run_11` | `bytes[11]` | little | spec | The typed-reference zero-body form stores the same prefix with a 51-byte typed shared-metadata reference at offset 32 |
| 32 | 1 | `shared_metadata_marker` | `u8` | little | spec | The typed-reference zero-body form stores the same prefix with a 51-byte typed shared-metadata reference at offset 32 |
| 33 | 8 | `shared_metadata_record` | `u64` | little | spec | The typed-reference zero-body form stores the same prefix with a 51-byte typed shared-metadata reference at offset 32 |
| 41 | 42 | `shared_metadata_field` | `bytes[42]` | little | spec | The typed-reference zero-body form stores the same prefix with a 51-byte typed shared-metadata reference at offset 32 |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `sheet_metal_rule_prefix`

Spec §3.1 · layout: byte offsets · size: not stated

This size-less byte record describes a sheet-metal rule record whose first entry is `Thickness`. Offsets are relative to the rule record's primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/sheet_metal_settings.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | After its indexed header the rule record stores ten zero bytes |
| 11 | 10 | `zero_run_11` | `bytes[10]` | little | spec | After its indexed header the rule record stores ten zero bytes |
| 21 | 4 | `entry_word` | `u32` | little | spec | a u32 rule word |
| 25 | 22 | `first_entry_name` | `bytes[22]` | little | spec | its 22-byte name starts at rule-record offset 25 |
| 47 | 8 | `thickness` | `f64` | little | spec | its value at offset 47 |

## `assembly_compact_wide_limit_owner_run`

Spec §Assembly operands · layout: byte offsets · size: not stated

This size-less byte record starts at scope offset 526 in the 806-, 850- and 861-byte compact wide Assemble scopes. Offsets are relative to the count. The count is 0 in the 806- and 861-byte scopes, which store no references; the 850-byte scope stores count 2 and both references.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_frames.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `limit_owner_count` | `u32` | little | spec | A u32 limit-owner count at offset 526 |
| 4 | 11 | `first_limit_owner_reference` | `bytes[11]` | little | spec | two limit owners, owner-local ordinals 4 and 5, at offsets 530 and 541 |
| 15 | 11 | `second_limit_owner_reference` | `bytes[11]` | little | spec | two limit owners, owner-local ordinals 4 and 5, at offsets 530 and 541 |

Cross-checked against code:

- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_frames.rs` — The run starts at scope offset 526.

## `assembly_wide_scope`

Spec §Assembly operands · layout: byte offsets · size: not stated

This size-less byte record describes the frame-sense prefix of the 945-byte scope. Offsets are relative to the scope's primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_alignment.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The 945-byte `Assemble` scope uses the wide operand-frame form |
| 20 | 1 | `wide_marker` | `u8` | little | spec | `u8 1` at offset 20 |
| 21 | 4 | `zero_run_21` | `bytes[4]` | little | spec | four zero bytes, the frame-sense byte at offset 25 |
| 25 | 1 | `frame_sense` | `u8` | little | spec | the frame-sense byte at offset 25 |
| 26 | 11 | `zero_run_26` | `bytes[11]` | little | spec | zero bytes at offsets 26 through 36 |

Unstated regions:

- `11..20` (9 B): Bytes 11 through 19 precede the wide marker and are not read for the frame sense.

## `assembly_rigid_as_built_scope`

Spec §Assembly operands · layout: byte offsets · size: 257 B

Offsets are relative to the primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The ordinary two-member rigid-group |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | The ordinary two-member rigid-group |
| 19 | 1 | `leading_block_presence` | `u8` | little | spec | The ordinary two-member rigid-group · value `0` |
| 20 | 1 | `property_block_presence` | `u8` | little | spec | The ordinary two-member rigid-group · value `0` |
| 21 | 1 | `zero_separator` | `u8` | little | spec | The ordinary two-member rigid-group · value `0` |
| 22 | 11 | `pathless_locator_reference` | `bytes[11]` | little | spec | The ordinary two-member rigid-group |
| 33 | 4 | `zero_run_33` | `bytes[4]` | little | spec | The ordinary two-member rigid-group |
| 37 | 4 | `locator_count` | `u32` | little | spec | The ordinary two-member rigid-group |
| 41 | 11 | `first_locator_reference` | `bytes[11]` | little | spec | The ordinary two-member rigid-group |
| 52 | 11 | `second_locator_reference` | `bytes[11]` | little | spec | The ordinary two-member rigid-group |
| 63 | 11 | `joint_reference` | `bytes[11]` | little | spec | The ordinary two-member rigid-group |
| 74 | 76 | `guid` | `bytes[76]` | little | spec | The ordinary two-member rigid-group |
| 150 | 3 | `zero_run_150` | `bytes[3]` | little | spec | The ordinary two-member rigid-group |
| 153 | 4 | `reference_count` | `u32` | little | spec | The ordinary two-member rigid-group |
| 157 | 4 | `history_state` | `u32` | little | spec | The ordinary two-member rigid-group |
| 161 | 20 | `kind` | `bytes[20]` | little | spec | The ordinary two-member rigid-group |
| 181 | 76 | `scope_tail` | `bytes[76]` | little | spec | The ordinary two-member rigid-group |

## `assembly_typed_rigid_group_scope`

Spec §Assembly operands · layout: byte offsets · size: 537 B

Offsets are relative to the primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 11 | 11 | `zero_run_11` | `bytes[11]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 22 | 51 | `pathless_locator_reference` | `bytes[51]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 73 | 4 | `zero_run_73` | `bytes[4]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 77 | 4 | `locator_count` | `u32` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 81 | 51 | `first_locator_reference` | `bytes[51]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 132 | 51 | `second_locator_reference` | `bytes[51]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 183 | 51 | `joint_reference` | `bytes[51]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 234 | 76 | `guid` | `bytes[76]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 310 | 3 | `zero_run_310` | `bytes[3]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 313 | 4 | `reference_count` | `u32` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 317 | 4 | `history_state` | `u32` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 321 | 20 | `kind` | `bytes[20]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |
| 341 | 196 | `scope_tail` | `bytes[196]` | little | spec | The typed two-member rigid-group scope is 537 bytes. |

## `assembly_compact_rigid_group_locator`

Spec §Assembly operands · layout: byte offsets · size: 180 B

Offsets are relative to the primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A version-3 compact rigid-group locator is 180 bytes with a matrix |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | A version-3 compact rigid-group locator is 180 bytes with a matrix |
| 21 | 1 | `construction_reference` | `u8` | little | spec | Its construction reference is null at offset 21 · value `0` |
| 22 | 1 | `identity_flag` | `u8` | little | spec | the identity flag is at offset 22 · value `0` |
| 23 | 128 | `transform` | `f64[16]` | little | spec | A version-3 compact rigid-group locator is 180 bytes with a matrix |
| 151 | 1 | `zero_151` | `u8` | little | spec | The matrix form has separator at offset 151 |
| 152 | 11 | `scope_backlink` | `bytes[11]` | little | spec | scope backlink at offset 152 |
| 163 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | wrapper reference at offset 163 |
| 174 | 4 | `discriminator` | `u32` | little | spec | discriminator at offset 174 |
| 178 | 2 | `zero_tail` | `bytes[2]` | little | spec | zero tail at offset 178 |

## `assembly_typed_rigid_group_identity_locator`

Spec §Assembly operands · layout: byte offsets · size: 181 B

Offsets are relative to the primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The identity form is 181 bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The identity form is 181 bytes |
| 21 | 51 | `construction_reference` | `bytes[51]` | little | spec | The identity form is 181 bytes |
| 72 | 1 | `identity_flag` | `u8` | little | spec | The identity form is 181 bytes |
| 73 | 51 | `scope_backlink` | `bytes[51]` | little | spec | The identity form is 181 bytes |
| 124 | 51 | `wrapper_reference` | `bytes[51]` | little | spec | The identity form is 181 bytes |
| 175 | 4 | `discriminator` | `u32` | little | spec | The identity form is 181 bytes |
| 179 | 2 | `zero_tail` | `bytes[2]` | little | spec | The identity form is 181 bytes |

## `assembly_typed_rigid_group_matrix_locator`

Spec §Assembly operands · layout: byte offsets · size: 309 B

Offsets are relative to the primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The matrix form is 309 bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The matrix form is 309 bytes |
| 21 | 51 | `construction_reference` | `bytes[51]` | little | spec | The matrix form is 309 bytes |
| 72 | 1 | `identity_flag` | `u8` | little | spec | The matrix form is 309 bytes |
| 73 | 128 | `transform` | `f64[16]` | little | spec | The matrix form is 309 bytes |
| 201 | 51 | `scope_backlink` | `bytes[51]` | little | spec | The matrix form is 309 bytes |
| 252 | 51 | `wrapper_reference` | `bytes[51]` | little | spec | The matrix form is 309 bytes |
| 303 | 4 | `discriminator` | `u32` | little | spec | The matrix form is 309 bytes |
| 307 | 2 | `zero_tail` | `bytes[2]` | little | spec | The matrix form is 309 bytes |

## `rectangular_pattern_direction_record`

Spec §Assembly operands · layout: byte offsets · size: 220 B

Offsets are relative to the direction record's primary indexed header. The paired header starts at offset 220.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | After the indexed header and ten zero bytes |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | After the indexed header and ten zero bytes |
| 21 | 4 | `selection_kind` | `u32` | little | spec | a u32 selection kind `8` or `16` at offset 21 |
| 25 | 24 | `origin` | `f64[3]` | little | spec | a three-component origin in centimetres at offset 25 |
| 49 | 24 | `displacement` | `f64[3]` | little | spec | a three-component displacement in centimetres at offset 49 |
| 73 | 16 | `zero_run_16` | `bytes[16]` | little | spec | Sixteen zero bytes follow the displacement. |
| 89 | 4 | `fixed_eight` | `u32` | little | spec | Offset 89 stores u32 `8` |
| 93 | 24 | `negative_ones` | `f64[3]` | little | spec | offsets 93, 101, and 109 store f64 `-1.0` |
| 117 | 4 | `selection_count` | `u32` | little | spec | Offset 117 stores u32 `1` |
| 121 | 11 | `selection_reference` | `bytes[11]` | little | spec | the marked selection-operand reference starts at offset 121, followed by two zero bytes |
| 132 | 2 | `zero_run_2` | `bytes[2]` | little | spec | the marked selection-operand reference starts at offset 121, followed by two zero bytes |
| 134 | 4 | `auxiliary_count` | `u32` | little | spec | Offset 134 stores u32 `1` |
| 138 | 11 | `auxiliary_reference` | `bytes[11]` | little | spec | a marked reference starts at offset 138 |
| 149 | 8 | `role` | `u64` | little | spec | Offset 149 stores u64 `0x0000000400000000`, followed by ten zero bytes. |
| 157 | 10 | `zero_run_after_role` | `bytes[10]` | little | spec | Offset 149 stores u64 `0x0000000400000000`, followed by ten zero bytes. |
| 167 | 4 | `opaque_index` | `u32` | little | spec | Offset 167 stores a nonzero u32 |
| 171 | 8 | `opaque_scalar` | `f64` | little | spec | offset 171 a finite f64 |
| 179 | 4 | `opaque_index_repeat` | `u32` | little | spec | offset 179 the same u32 |
| 183 | 11 | `index_plus_two_reference` | `bytes[11]` | little | spec | Marked references to the direction record's index plus two and plus one start at offsets 183 and 197 |
| 194 | 1 | `separator_flag` | `u8` | little | spec | `u8 1` and two zero bytes separate them |
| 195 | 2 | `separator_zero_run` | `bytes[2]` | little | spec | `u8 1` and two zero bytes separate them |
| 197 | 11 | `index_plus_one_reference` | `bytes[11]` | little | spec | Marked references to the direction record's index plus two and plus one start at offsets 183 and 197 |
| 208 | 1 | `scope_separator` | `u8` | little | spec | One zero byte and the marked scope reference at offset 209 close the frame. |
| 209 | 11 | `scope_reference` | `bytes[11]` | little | spec | One zero byte and the marked scope reference at offset 209 close the frame. |

## `extrude_selection_member_long_tail`

Spec §3.1 · layout: byte offsets · size: 14 B

Offsets are relative to the end of the 190-byte selection-member frame.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `leading_zero` | `u8` | little | spec | byte `00`, u32 `2`, u64 `1`, and byte `00` · value `0` |
| 1 | 4 | `kind` | `u32` | little | spec | byte `00`, u32 `2`, u64 `1`, and byte `00` · value `2` |
| 5 | 8 | `value` | `u64` | little | spec | byte `00`, u32 `2`, u64 `1`, and byte `00` · value `1` |
| 13 | 1 | `trailing_zero` | `u8` | little | spec | byte `00`, u32 `2`, u64 `1`, and byte `00` · value `0` |

## `cut_paste_prefix`

Spec §3.1 · layout: byte offsets · size: 72 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/occurrence_transfer.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The compact CutPaste prefix |
| 11 | 10 | `zero_run` | `bytes[10]` | little | spec | The compact CutPaste prefix |
| 21 | 11 | `removal_reference` | `bytes[11]` | little | spec | The compact CutPaste prefix |
| 32 | 11 | `companion_reference` | `bytes[11]` | little | spec | The compact CutPaste prefix |
| 43 | 1 | `companion_flag` | `u8` | little | spec | The compact CutPaste prefix |
| 44 | 11 | `second_companion_reference` | `bytes[11]` | little | spec | The compact CutPaste prefix |
| 55 | 4 | `zero_run_4` | `bytes[4]` | little | spec | The compact CutPaste prefix |
| 59 | 11 | `relation_reference` | `bytes[11]` | little | spec | The compact CutPaste prefix |
| 70 | 1 | `placement_flag` | `u8` | little | spec | The compact CutPaste prefix |
| 71 | 1 | `identity_flag` | `u8` | little | spec | The compact CutPaste prefix |

## `copy_paste_prefix`

Spec §3.1 · layout: byte offsets · size: 190 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/occurrence_transfer.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The compact CopyPaste prefix |
| 11 | 11 | `zero_run` | `bytes[11]` | little | spec | The compact CopyPaste prefix |
| 22 | 11 | `companion_reference` | `bytes[11]` | little | spec | The compact CopyPaste prefix |
| 33 | 1 | `zero_separator` | `u8` | little | spec | The compact CopyPaste prefix |
| 34 | 128 | `source_transform` | `f64[16]` | little | spec | The compact CopyPaste prefix |
| 162 | 11 | `second_companion_reference` | `bytes[11]` | little | spec | The compact CopyPaste prefix |
| 173 | 4 | `zero_run_4` | `bytes[4]` | little | spec | The compact CopyPaste prefix |
| 177 | 11 | `relation_reference` | `bytes[11]` | little | spec | The compact CopyPaste prefix |
| 188 | 1 | `placement_flag` | `u8` | little | spec | The compact CopyPaste prefix |
| 189 | 1 | `identity_flag` | `u8` | little | spec | The compact CopyPaste prefix |

## `paste_occurrence_relation`

Spec §3.1 · layout: byte offsets · size: 87 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/occurrence_transfer.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The relation has a 57-byte primary record |
| 11 | 10 | `zero_run` | `bytes[10]` | little | spec | The relation has a 57-byte primary record |
| 21 | 11 | `occurrence_reference` | `bytes[11]` | little | spec | The relation has a 57-byte primary record |
| 32 | 2 | `zero_run_2` | `bytes[2]` | little | spec | The relation has a 57-byte primary record |
| 34 | 11 | `companion_reference` | `bytes[11]` | little | spec | The relation has a 57-byte primary record |
| 45 | 1 | `zero_separator` | `u8` | little | spec | The relation has a 57-byte primary record |
| 46 | 11 | `scope_reference` | `bytes[11]` | little | spec | The relation has a 57-byte primary record |
| 57 | 11 | `paired_header` | `bytes[11]` | little | spec | The relation has a 57-byte primary record |
| 68 | 8 | `zero_run_8` | `bytes[8]` | little | spec | The relation has a 57-byte primary record |
| 76 | 1 | `owner_marker` | `u8` | little | spec | The relation has a 57-byte primary record |
| 77 | 8 | `owner_collection` | `u64` | little | spec | The relation has a 57-byte primary record |
| 85 | 2 | `owner_zero_run` | `bytes[2]` | little | spec | The relation has a 57-byte primary record |

## `remove_instance_prefix`

Spec §3.1 · layout: byte offsets · size: 135 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/occurrence_transfer.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The RemoveInstance prefix |
| 11 | 22 | `zero_run` | `bytes[22]` | little | spec | The RemoveInstance prefix |
| 33 | 11 | `source_relation_reference` | `bytes[11]` | little | spec | The RemoveInstance prefix |
| 44 | 76 | `occurrence_key` | `bytes[76]` | little | spec | The RemoveInstance prefix |
| 120 | 7 | `zero_run_7` | `bytes[7]` | little | spec | The RemoveInstance prefix |
| 127 | 4 | `history_state` | `u32` | little | spec | The RemoveInstance prefix |
| 131 | 4 | `kind_length` | `u32` | little | spec | The RemoveInstance prefix |

## `paste_context_owner_tail`

Spec §3.1 · layout: byte offsets · size: 52 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/occurrence_transfer.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `collection_reference` | `bytes[11]` | little | spec | The context owner tail is 52 bytes |
| 11 | 11 | `companion_reference` | `bytes[11]` | little | spec | The context owner tail is 52 bytes |
| 22 | 11 | `paired_header` | `bytes[11]` | little | spec | The context owner tail is 52 bytes |
| 33 | 8 | `zero_run_8` | `bytes[8]` | little | spec | The context owner tail is 52 bytes |
| 41 | 11 | `component_reference` | `bytes[11]` | little | spec | The context owner tail is 52 bytes |

## `component_insert_identity_scope_272_prefix`

Spec §3.1 · layout: byte offsets · size: 126 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/component_constructions.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | fourteen zero bytes at offsets 11 through 24 |
| 11 | 14 | `zero_prefix` | `bytes[14]` | little | spec | fourteen zero bytes at offsets 11 through 24 |
| 25 | 8 | `occurrence_identity` | `u64` | little | spec | its occurrence identity u64 at offset 25 |
| 33 | 4 | `zero_after_identity` | `bytes[4]` | little | spec | four zero bytes at offset 33 |
| 37 | 5 | `relation_reference` | `bytes[5]` | little | spec | its marked relation reference at offset 37 |
| 42 | 6 | `zero_after_relation` | `bytes[6]` | little | spec | six zero bytes at offset 42 |
| 48 | 2 | `identity_markers` | `bytes[2]` | little | spec | identity bytes `01 01` at offset 48 |
| 50 | 76 | `null_guid` | `bytes[76]` | little | spec | The LP-UTF16 null GUID starts at offset 50 |

## `component_insert_identity_scope_276_prefix`

Spec §3.1 · layout: byte offsets · size: 130 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/component_constructions.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | All subsequent fields move four bytes later |
| 11 | 18 | `prologue` | `bytes[18]` | little | spec | All subsequent fields move four bytes later |
| 29 | 8 | `occurrence_identity` | `u64` | little | spec | All subsequent fields move four bytes later |
| 37 | 4 | `zero_after_identity` | `bytes[4]` | little | spec | All subsequent fields move four bytes later |
| 41 | 5 | `relation_reference` | `bytes[5]` | little | spec | All subsequent fields move four bytes later |
| 46 | 6 | `zero_after_relation` | `bytes[6]` | little | spec | All subsequent fields move four bytes later |
| 52 | 2 | `identity_markers` | `bytes[2]` | little | spec | All subsequent fields move four bytes later |
| 54 | 76 | `null_guid` | `bytes[76]` | little | spec | All subsequent fields move four bytes later |

## `component_insert_identity_carrier`

Spec §3.1 · layout: byte offsets · size: 420 B

The sequential legacy target-reference grammar and the fixed identity insertion members determine these offsets.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 19 | `indexed_entity_and_empty_name` | `bytes[19]` | little | spec | The 420-byte identity insertion carrier |
| 19 | 1 | `path_marker` | `u8` | little | spec | The 420-byte identity insertion carrier |
| 20 | 4 | `path_count` | `u32` | little | spec | The 420-byte identity insertion carrier |
| 24 | 60 | `target_reference` | `bytes[60]` | little | spec | The 420-byte identity insertion carrier |
| 84 | 4 | `instance_discriminator` | `u32` | little | spec | The 420-byte identity insertion carrier |
| 88 | 4 | `member_count` | `u32` | little | spec | The 420-byte identity insertion carrier |
| 92 | 1 | `zero_after_count` | `u8` | little | spec | The 420-byte identity insertion carrier |
| 93 | 1 | `metadata_boolean` | `u8` | little | spec | The 420-byte identity insertion carrier |
| 94 | 1 | `zero_after_boolean` | `u8` | little | spec | The 420-byte identity insertion carrier |
| 95 | 4 | `metadata_count` | `u32` | little | spec | The 420-byte identity insertion carrier |
| 99 | 76 | `placement_metadata_guid` | `bytes[76]` | little | spec | The 420-byte identity insertion carrier |
| 175 | 76 | `tracking_link_guid` | `bytes[76]` | little | spec | The 420-byte identity insertion carrier |
| 251 | 1 | `metadata_marker` | `u8` | little | spec | The 420-byte identity insertion carrier |
| 252 | 8 | `opaque_metadata_token` | `u64` | little | spec | The 420-byte identity insertion carrier |
| 260 | 7 | `metadata_tail` | `bytes[7]` | little | spec | The 420-byte identity insertion carrier |
| 267 | 60 | `repeated_target_reference` | `bytes[60]` | little | spec | The 420-byte identity insertion carrier |
| 327 | 1 | `identity_flag` | `u8` | little | spec | The 420-byte identity insertion carrier |
| 328 | 4 | `zero_after_identity` | `u32` | little | spec | The 420-byte identity insertion carrier |
| 332 | 76 | `occurrence_role` | `bytes[76]` | little | spec | The 420-byte identity insertion carrier |
| 408 | 12 | `closure` | `bytes[12]` | little | spec | The 420-byte identity insertion carrier |

## `emboss_scope_prefix`

Spec §3.1 · layout: byte offsets · size: 156 B

Offsets are relative to the primary indexed header. The null GUID and common reference table follow this prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/emboss.rs`
- `crates/cadmpeg-codec-f3d/src/records/feature/emboss.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 11 | 9 | `zero_run` | `bytes[9]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. · value `[0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 20 | 8 | `control_words` | `u32[2]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 28 | 11 | `depth_owner_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 39 | 11 | `auxiliary_group_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 50 | 11 | `horizontal_owner_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 61 | 4 | `native_pair_count` | `u32` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. · value `1` |
| 65 | 16 | `native_pair` | `u64[2]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 81 | 11 | `rotation_owner_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 92 | 11 | `face_group_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 103 | 11 | `vertical_owner_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 114 | 4 | `profile_group_count` | `u32` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. · value `1` |
| 118 | 11 | `profile_group_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |
| 129 | 12 | `zero_tail` | `bytes[12]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. · value `[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 141 | 4 | `face_group_count` | `u32` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. · value `1` |
| 145 | 11 | `repeated_face_group_reference` | `bytes[11]` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. |

Cross-checked against code:

- `crates/cadmpeg-codec-f3d/src/records/feature/emboss.rs` — Prefix-field order is Depth, auxiliary group, Horizontal Distance, Rotation Angle, target-face group, Vertical Distance, profile group. This table order selects the sole-member operand grammar.

## `emboss_scope_reference_table_prefix`

Spec §3.1 · layout: byte offsets · size: 239 B

The operation prefix, null GUID, and separator precede the common reference count.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/emboss.rs`
- `crates/cadmpeg-codec-f3d/src/records/feature/emboss.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 156 | `operation_prefix` | `bytes[156]` | little | spec | Its operation prefix is the `emboss_scope_prefix` record |
| 156 | 4 | `null_guid_count` | `u32` | little | spec | A 36-code-unit LP-UTF16 null GUID and three zero bytes separate the prefix · value `36` |
| 160 | 72 | `null_guid` | `u16[36]` | little | spec | A 36-code-unit LP-UTF16 null GUID and three zero bytes separate the prefix |
| 232 | 3 | `zero_separator` | `bytes[3]` | little | spec | A 36-code-unit LP-UTF16 null GUID and three zero bytes separate the prefix · value `[0, 0, 0]` |
| 235 | 4 | `reference_count` | `u32` | little | spec | The `Emboss` fixed-prefix form has nine ordered references. · value `9` |

## `rectangular_pattern_spacing_direction_record`

Spec §Assembly operands · layout: byte offsets · size: 221 B

Offsets are relative to the direction record's primary indexed header. The paired header starts at offset 221.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The 221-byte spacing direction form |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The 221-byte spacing direction form |
| 21 | 4 | `selection_kind` | `u32` | little | spec | The 221-byte spacing direction form |
| 25 | 24 | `origin` | `f64[3]` | little | spec | The 221-byte spacing direction form |
| 49 | 24 | `displacement` | `f64[3]` | little | spec | The 221-byte spacing direction form |
| 73 | 16 | `zero_run_16` | `bytes[16]` | little | spec | The 221-byte spacing direction form |
| 89 | 4 | `distance_type` | `u32` | little | spec | The 221-byte spacing direction form · value `7` |
| 93 | 1 | `spacing_flag` | `u8` | little | spec | The 221-byte spacing direction form · value `0` |
| 94 | 24 | `negative_ones` | `f64[3]` | little | spec | The 221-byte spacing direction form |
| 118 | 4 | `selection_count` | `u32` | little | spec | The 221-byte spacing direction form |
| 122 | 11 | `selection_reference` | `bytes[11]` | little | spec | The 221-byte spacing direction form |
| 133 | 2 | `zero_run_2` | `bytes[2]` | little | spec | The 221-byte spacing direction form |
| 135 | 4 | `auxiliary_count` | `u32` | little | spec | The 221-byte spacing direction form |
| 139 | 11 | `auxiliary_reference` | `bytes[11]` | little | spec | The 221-byte spacing direction form |
| 150 | 8 | `role` | `u64` | little | spec | The 221-byte spacing direction form |
| 158 | 10 | `zero_run_after_role` | `bytes[10]` | little | spec | The 221-byte spacing direction form |
| 168 | 4 | `opaque_index` | `u32` | little | spec | The 221-byte spacing direction form |
| 172 | 8 | `opaque_scalar` | `f64` | little | spec | The 221-byte spacing direction form |
| 180 | 4 | `opaque_index_repeat` | `u32` | little | spec | The 221-byte spacing direction form |
| 184 | 11 | `index_plus_two_reference` | `bytes[11]` | little | spec | The 221-byte spacing direction form |
| 195 | 1 | `separator_flag` | `u8` | little | spec | The 221-byte spacing direction form |
| 196 | 2 | `separator_zero_run` | `bytes[2]` | little | spec | The 221-byte spacing direction form |
| 198 | 11 | `index_plus_one_reference` | `bytes[11]` | little | spec | The 221-byte spacing direction form |
| 209 | 1 | `scope_separator` | `u8` | little | spec | The 221-byte spacing direction form |
| 210 | 11 | `scope_reference` | `bytes[11]` | little | spec | The 221-byte spacing direction form |

## `rectangular_pattern_payload_prefix`

Spec §Assembly operands · layout: byte offsets · size: 133 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 11 | 8 | `zero_head` | `bytes[8]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 19 | 1 | `flag` | `u8` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 20 | 4 | `count` | `u32` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 24 | 11 | `u_count` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 35 | 4 | `zero_middle` | `bytes[4]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 39 | 11 | `v_count` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 50 | 9 | `zero_tail` | `bytes[9]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 59 | 4 | `sentinel` | `u32` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 63 | 24 | `v_direction` | `f64[3]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 87 | 11 | `u_distance` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 98 | 11 | `v_distance` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 109 | 2 | `zero_distance` | `bytes[2]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 111 | 11 | `v_count_repeat` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 122 | 11 | `u_direction` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |

## `rectangular_pattern_payload_suffix`

Spec §Assembly operands · layout: byte offsets · size: 37 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `u_count` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 11 | 11 | `scope` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 22 | 11 | `seed_group` | `bytes[11]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |
| 33 | 4 | `zero_tail` | `bytes[4]` | little | spec | The payload has a 133-byte prefix and a 37-byte suffix. |

## `sketch_geometry_ordinal_collection_reference`

Spec §3.1 · layout: byte offsets · size: 11 B

The reference starts at offset 36 relative to the Geometry-member indexed header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | byte 36 is `01` · value `1` |
| 1 | 8 | `target` | `u64` | little | spec | its u64 target at member offset 37 names the ordinal collection |
| 9 | 2 | `flags` | `bytes[2]` | little | spec | Byte 36 opens a same-segment marked reference; its u64 target at member offset 37 names the ordinal collection. |

## `sketch_ordinal_collection_prefix`

Spec §3.1 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/sketch.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/sketch_owners.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its indexed header is followed by nine zero bytes, a property block carrying `crv_tag_id_idx` |
| 11 | 9 | `zero_run` | `bytes[9]` | little | spec | Its indexed header is followed by nine zero bytes, a property block carrying `crv_tag_id_idx` |

## `assembly_as_built_scope_378`

Spec §Assembly operands · layout: byte offsets · size: 378 B

Offsets are relative to the primary indexed header. K = 0.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_alignment.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 43 | 4 | `additional_owner_count` | `u32` | little | spec | offset 43 · value `0` |
| 47 | 4 | `locator_count` | `u32` | little | spec | stores a u32 count of two |
| 51 | 11 | `first_locator_reference` | `bytes[11]` | little | spec | two marked same-segment operand-path-locator references |
| 62 | 11 | `second_locator_reference` | `bytes[11]` | little | spec | two marked same-segment operand-path-locator references |
| 163 | 4 | `reference_count` | `u32` | little | spec | The reference-table counts are at offsets 163, 185, and 196 · value `9` |
| 167 | 99 | `reference_members` | `bytes[99]` | little | derived | Each marked reference has eleven bytes; K = 0. |

Unstated regions:

- `0..43` (43 B): Prefix before the owner count.
- `73..163` (90 B): Scope fields before the reference table.
- `266..378` (112 B): Scope suffix after the reference table.

## `assembly_as_built_scope_422`

Spec §Assembly operands · layout: byte offsets · size: 422 B

Offsets are relative to the primary indexed header. K = 2.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_alignment.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 43 | 4 | `additional_owner_count` | `u32` | little | spec | offset 43 · value `2` |
| 47 | 22 | `additional_owner_references` | `bytes[22]` | little | spec | `47 + 11k` |
| 69 | 4 | `locator_count` | `u32` | little | spec | stores a u32 count of two |
| 73 | 11 | `first_locator_reference` | `bytes[11]` | little | spec | two marked same-segment operand-path-locator references |
| 84 | 11 | `second_locator_reference` | `bytes[11]` | little | spec | two marked same-segment operand-path-locator references |
| 185 | 4 | `reference_count` | `u32` | little | spec | The reference-table counts are at offsets 163, 185, and 196 · value `11` |
| 189 | 121 | `reference_members` | `bytes[121]` | little | derived | Each marked reference has eleven bytes; K = 2. |

Unstated regions:

- `0..43` (43 B): Prefix before the owner count.
- `95..185` (90 B): Scope fields before the reference table.
- `310..422` (112 B): Scope suffix after the reference table.

## `assembly_as_built_scope_444`

Spec §Assembly operands · layout: byte offsets · size: 444 B

Offsets are relative to the primary indexed header. K = 3.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_alignment.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 43 | 4 | `additional_owner_count` | `u32` | little | spec | offset 43 · value `3` |
| 47 | 33 | `additional_owner_references` | `bytes[33]` | little | spec | `47 + 11k` |
| 80 | 4 | `locator_count` | `u32` | little | spec | stores a u32 count of two |
| 84 | 11 | `first_locator_reference` | `bytes[11]` | little | spec | two marked same-segment operand-path-locator references |
| 95 | 11 | `second_locator_reference` | `bytes[11]` | little | spec | two marked same-segment operand-path-locator references |
| 196 | 4 | `reference_count` | `u32` | little | spec | The reference-table counts are at offsets 163, 185, and 196 · value `12` |
| 200 | 132 | `reference_members` | `bytes[132]` | little | derived | Each marked reference has eleven bytes; K = 3. |

Unstated regions:

- `0..43` (43 B): Prefix before the owner count.
- `106..196` (90 B): Scope fields before the reference table.
- `332..444` (112 B): Scope suffix after the reference table.

## `assembly_as_built_scope_owner_run`

Spec §Assembly operands · layout: byte offsets · size: not stated

This size-less byte record gives the common count and owner-run start in the 378-, 422-, and 444-byte As-built scopes. Offsets are relative to the primary indexed header. The count K controls the number of eleven-byte marked references; K = 0 stores no owner reference.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_alignment.rs`
- `crates/cadmpeg-codec-f3d/src/validate.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 43 | 4 | `additional_owner_count` | `u32` | little | spec | offset 43 |
| 47 | 11 | `additional_owner_references` | `bytes[11]` | little | spec | `47 + 11k` |

Unstated regions:

- `0..43` (43 B): Prefix before the common owner count.

## `sketch_owner_collection_legacy_list_prefix`

Spec §3.1 · layout: byte offsets · size: 6 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/sketch.rs`
- `crates/cadmpeg-codec-f3d/src/design/decode/sketch_owners.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 6 | `prefix` | `bytes[6]` | little | spec | Version `3` has six bytes after this property block and before the member list. |

## `sketch_owner_reference`

Spec §3.1 · layout: byte offsets · size: 11 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/sketch_owners.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | a one-byte marker |
| 1 | 8 | `target` | `u64` | little | spec | a u64 target |
| 9 | 2 | `flags` | `bytes[2]` | little | spec | two zero flag bytes |

## `sketch_owner_typed_reference`

Spec §3.1 · layout: byte offsets · size: 51 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/sketch_owners.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | a one-byte marker |
| 1 | 8 | `target` | `u64` | little | spec | a u64 target |
| 9 | 4 | `type_guid_length` | `u32` | little | spec | an additional u32 length `36` |
| 13 | 36 | `type_guid` | `bytes[36]` | little | spec | 36 ASCII type-GUID bytes between the target and the flags |
| 49 | 2 | `flags` | `bytes[2]` | little | spec | two zero flag bytes |

## `local_component_occurrence_typed_prefix`

Spec §3.1 · layout: byte offsets · size: 289 B

The separated identity and matrix frames have one zero separator before the repeated reference.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 11 | 8 | `empty_instance_name` | `bytes[8]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 19 | 1 | `path_marker` | `u8` | little | spec | Typed local component occurrences use local references with inline type GUIDs. · value `1` |
| 20 | 4 | `path_count` | `u32` | little | spec | Typed local component occurrences use local references with inline type GUIDs. · value `1` |
| 24 | 51 | `component_reference` | `bytes[51]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 75 | 4 | `path_discriminator` | `u32` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 79 | 1 | `path_terminator` | `u8` | little | spec | Typed local component occurrences use local references with inline type GUIDs. · value `0` |
| 80 | 4 | `occurrence_ordinal` | `u32` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 84 | 76 | `carrier_guid` | `bytes[76]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 160 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 236 | 1 | `separator` | `u8` | little | spec | Typed local component occurrences use local references with inline type GUIDs. · value `0` |
| 237 | 51 | `component_reference_repeat` | `bytes[51]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 288 | 1 | `identity_flag` | `u8` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |

## `local_component_occurrence_typed_compact_prefix`

Spec §3.1 · layout: byte offsets · size: 288 B

The 348-byte identity and 476-byte matrix frames have no separator before the repeated reference.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 11 | 8 | `empty_instance_name` | `bytes[8]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 19 | 1 | `path_marker` | `u8` | little | spec | Typed local component occurrences use local references with inline type GUIDs. · value `1` |
| 20 | 4 | `path_count` | `u32` | little | spec | Typed local component occurrences use local references with inline type GUIDs. · value `1` |
| 24 | 51 | `component_reference` | `bytes[51]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 75 | 4 | `path_discriminator` | `u32` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 79 | 1 | `path_terminator` | `u8` | little | spec | Typed local component occurrences use local references with inline type GUIDs. · value `0` |
| 80 | 4 | `occurrence_ordinal` | `u32` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 84 | 76 | `carrier_guid` | `bytes[76]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 160 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 236 | 51 | `component_reference_repeat` | `bytes[51]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 287 | 1 | `identity_flag` | `u8` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |

## `local_component_occurrence_typed_identity_tail`

Spec §3.1 · layout: byte offsets · size: 60 B

Relative to the end of the typed identity prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 9 | `zero_run` | `bytes[9]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 9 | 51 | `owner_reference` | `bytes[51]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |

## `local_component_occurrence_typed_matrix_tail`

Spec §3.1 · layout: byte offsets · size: 188 B

Relative to the end of the typed prefix.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 128 | `transform` | `f64[16]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 128 | 9 | `zero_run` | `bytes[9]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |
| 137 | 51 | `owner_reference` | `bytes[51]` | little | spec | Typed local component occurrences use local references with inline type GUIDs. |

## `local_component_occurrence_typed_attributed_prefix`

Spec §3.1 · layout: byte offsets · size: 84 B

The counted attribute run follows the typed component reference and the path discriminator.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An attributed local component occurrence also permits a typed component reference. |
| 11 | 8 | `empty_instance_name` | `bytes[8]` | little | spec | An attributed local component occurrence also permits a typed component reference. |
| 19 | 1 | `path_marker` | `u8` | little | spec | An attributed local component occurrence also permits a typed component reference. · value `1` |
| 20 | 4 | `path_count` | `u32` | little | spec | An attributed local component occurrence also permits a typed component reference. · value `1` |
| 24 | 51 | `component_reference` | `bytes[51]` | little | spec | An attributed local component occurrence also permits a typed component reference. |
| 75 | 4 | `path_discriminator` | `u32` | little | spec | An attributed local component occurrence also permits a typed component reference. |
| 79 | 1 | `attributes_marker` | `u8` | little | spec | An attributed local component occurrence also permits a typed component reference. · value `1` |
| 80 | 4 | `attribute_count` | `u32` | little | spec | An attributed local component occurrence also permits a typed component reference. |

## `root_instance_empty_path_prefix`

Spec §3.1 · layout: byte offsets · size: 36 B

The path has no component-definition reference.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | A named root-instance record can store an empty path. |
| 11 | 4 | `leading_zero` | `u32` | little | spec | A named root-instance record can store an empty path. · value `0` |
| 15 | 4 | `instance_name_length` | `u32` | little | spec | A named root-instance record can store an empty path. · value `12` |
| 19 | 12 | `instance_name` | `bytes[12]` | little | spec | A named root-instance record can store an empty path. |
| 31 | 1 | `path_marker` | `u8` | little | spec | A named root-instance record can store an empty path. · value `1` |
| 32 | 4 | `path_count` | `u32` | little | spec | A named root-instance record can store an empty path. · value `0` |

## `root_instance_empty_path_frame`

Spec §3.1 · layout: byte offsets · size: 255 B

The typed reference after the two GUIDs names the root component definition.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. |
| 11 | 4 | `leading_zero` | `u32` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. · value `0` |
| 15 | 4 | `instance_name_length` | `u32` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. · value `12` |
| 19 | 12 | `instance_name` | `bytes[12]` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. |
| 31 | 1 | `path_marker` | `u8` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. · value `1` |
| 32 | 4 | `path_count` | `u32` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. · value `0` |
| 36 | 1 | `separator` | `u8` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. · value `0` |
| 37 | 4 | `word` | `u32` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. · value `2` |
| 41 | 76 | `carrier_guid` | `bytes[76]` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. |
| 117 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. |
| 193 | 51 | `component_reference` | `bytes[51]` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. |
| 244 | 1 | `identity_flag` | `u8` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. · value `1` |
| 245 | 10 | `zero_tail` | `bytes[10]` | little | spec | The empty-path typed identity root-instance frame is 255 bytes. |

## `root_instance_empty_path_separated_frame`

Spec §3.1 · layout: byte offsets · size: 256 B

The typed reference after the two GUIDs names the root component definition.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. |
| 11 | 4 | `leading_zero` | `u32` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `0` |
| 15 | 4 | `instance_name_length` | `u32` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `12` |
| 19 | 12 | `instance_name` | `bytes[12]` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. |
| 31 | 1 | `path_marker` | `u8` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `1` |
| 32 | 4 | `path_count` | `u32` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `0` |
| 36 | 1 | `separator` | `u8` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `0` |
| 37 | 4 | `word` | `u32` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `2` |
| 41 | 76 | `carrier_guid` | `bytes[76]` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. |
| 117 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. |
| 193 | 1 | `reference_separator` | `u8` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `0` |
| 194 | 51 | `component_reference` | `bytes[51]` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. |
| 245 | 1 | `identity_flag` | `u8` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. · value `1` |
| 246 | 10 | `zero_tail` | `bytes[10]` | little | spec | The separated empty-path typed identity root-instance frame is 256 bytes. |

## `assembly_root_operand_path`

Spec §Assembly operands · layout: byte offsets · size: 277 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 15 | `indexed_header` | `bytes[15]` | little | spec | A typed root operand path is a 277-byte frame. |
| 15 | 6 | `zero_prefix` | `bytes[6]` | little | spec | A typed root operand path is a 277-byte frame. |
| 21 | 4 | `count` | `u32` | little | spec | A typed root operand path is a 277-byte frame. |
| 25 | 76 | `root_key` | `bytes[76]` | little | spec | A typed root operand path is a 277-byte frame. |
| 101 | 4 | `empty_reference` | `u32` | little | spec | A typed root operand path is a 277-byte frame. |
| 105 | 8 | `constant_one` | `u64` | little | spec | A typed root operand path is a 277-byte frame. |
| 113 | 76 | `first_identity` | `bytes[76]` | little | spec | A typed root operand path is a 277-byte frame. |
| 189 | 76 | `second_identity` | `bytes[76]` | little | spec | A typed root operand path is a 277-byte frame. |
| 265 | 4 | `constant_two` | `u32` | little | spec | A typed root operand path is a 277-byte frame. |
| 269 | 8 | `zero_tail` | `bytes[8]` | little | spec | A typed root operand path is a 277-byte frame. |

## `assembly_wide_operand_path_identity_locator`

Spec §Assembly operands · layout: byte offsets · size: 97 B

Offsets are relative to the locator indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 68 | 1 | `zero_separator` | `u8` | little | spec | The wide locator identity frame is 97 bytes. |
| 69 | 11 | `scope_backlink` | `bytes[11]` | little | spec | The wide locator identity frame is 97 bytes. |
| 80 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | The wide locator identity frame is 97 bytes. |
| 91 | 4 | `discriminator` | `u32` | little | spec | The wide locator identity frame is 97 bytes. |
| 95 | 2 | `zero_tail` | `bytes[2]` | little | spec | The wide locator identity frame is 97 bytes. |

Unstated regions:

- `0..68` (68 B): The wide locator prefix precedes the optional matrix.

## `assembly_wide_operand_path_matrix_locator`

Spec §Assembly operands · layout: byte offsets · size: 225 B

Offsets are relative to the locator indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 68 | 128 | `transform` | `f64[16]` | little | spec | The wide locator matrix frame is 225 bytes. |
| 196 | 1 | `zero_separator` | `u8` | little | spec | The wide locator matrix frame is 225 bytes. |
| 197 | 11 | `scope_backlink` | `bytes[11]` | little | spec | The wide locator matrix frame is 225 bytes. |
| 208 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | The wide locator matrix frame is 225 bytes. |
| 219 | 4 | `discriminator` | `u32` | little | spec | The wide locator matrix frame is 225 bytes. |
| 223 | 2 | `zero_tail` | `bytes[2]` | little | spec | The wide locator matrix frame is 225 bytes. |

Unstated regions:

- `0..68` (68 B): The wide locator prefix precedes the optional matrix.

## `ordinary_rigid_group_tail_76`

Spec §Assembly operands · layout: byte offsets · size: 76 B

Offsets are relative to the end of the LP-UTF16 kind.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `ordinal` | `u32` | little | spec | The 76-byte tail |
| 4 | 15 | `zero_run` | `bytes[15]` | little | spec | The 76-byte tail |
| 19 | 11 | `first_reference` | `bytes[11]` | little | spec | The 76-byte tail |
| 30 | 4 | `previous_history_state_id` | `u32` | little | spec | The 76-byte tail |
| 34 | 4 | `first_lane` | `u32` | little | spec | The 76-byte tail |
| 38 | 8 | `scalar` | `f64` | little | spec | The 76-byte tail |
| 46 | 4 | `second_lane` | `u32` | little | spec | The 76-byte tail |
| 50 | 11 | `second_reference` | `bytes[11]` | little | spec | The 76-byte tail |
| 61 | 1 | `reference_prefix` | `u8` | little | spec | The 76-byte tail · value `0` |
| 62 | 1 | `reference_prefix_payload` | `bytes[1]` | little | spec | The 76-byte tail |
| 63 | 11 | `third_reference` | `bytes[11]` | little | spec | The 76-byte tail |
| 74 | 2 | `zero_tail` | `bytes[2]` | little | spec | The 76-byte tail |

## `ordinary_rigid_group_tail_88`

Spec §Assembly operands · layout: byte offsets · size: 88 B

Offsets are relative to the end of the LP-UTF16 kind.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `ordinal` | `u32` | little | spec | The 88-byte tail |
| 4 | 15 | `zero_run` | `bytes[15]` | little | spec | The 88-byte tail |
| 19 | 11 | `first_reference` | `bytes[11]` | little | spec | The 88-byte tail |
| 30 | 11 | `additional_reference` | `bytes[11]` | little | spec | The 88-byte tail |
| 41 | 4 | `previous_history_state_id` | `u32` | little | spec | The 88-byte tail |
| 45 | 4 | `first_lane` | `u32` | little | spec | The 88-byte tail |
| 49 | 8 | `scalar` | `f64` | little | spec | The 88-byte tail |
| 57 | 4 | `second_lane` | `u32` | little | spec | The 88-byte tail |
| 61 | 11 | `second_reference` | `bytes[11]` | little | spec | The 88-byte tail |
| 72 | 1 | `reference_prefix` | `u8` | little | spec | The 88-byte tail · value `1` |
| 73 | 2 | `reference_prefix_payload` | `bytes[2]` | little | spec | The 88-byte tail |
| 75 | 11 | `third_reference` | `bytes[11]` | little | spec | The 88-byte tail |
| 86 | 2 | `zero_tail` | `bytes[2]` | little | spec | The 88-byte tail |

## `ordinary_rigid_group_tail_77`

Spec §Assembly operands · layout: byte offsets · size: 77 B

Offsets are relative to the kind end; the optional reference and field tag select the 77-byte closure independently.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `ordinal` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 4 | 15 | `zero_run` | `bytes[15]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 19 | 11 | `first_reference` | `bytes[11]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 30 | 4 | `previous_history_state_id` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 34 | 4 | `first_lane` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 38 | 8 | `scalar` | `f64` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 46 | 4 | `second_lane` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 50 | 11 | `second_reference` | `bytes[11]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 61 | 1 | `reference_prefix` | `u8` | little | derived | Apply the independent reference and field-width choices to the tail grammar. · value `1` |
| 62 | 2 | `reference_prefix_payload` | `bytes[2]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 64 | 11 | `third_reference` | `bytes[11]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 75 | 2 | `zero_tail` | `bytes[2]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |

## `ordinary_rigid_group_tail_87`

Spec §Assembly operands · layout: byte offsets · size: 87 B

Offsets are relative to the kind end; the optional reference and field tag select the 87-byte closure independently.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `ordinal` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 4 | 15 | `zero_run` | `bytes[15]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 19 | 11 | `first_reference` | `bytes[11]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 30 | 11 | `additional_reference` | `bytes[11]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 41 | 4 | `previous_history_state_id` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 45 | 4 | `first_lane` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 49 | 8 | `scalar` | `f64` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 57 | 4 | `second_lane` | `u32` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 61 | 11 | `second_reference` | `bytes[11]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 72 | 1 | `reference_prefix` | `u8` | little | derived | Apply the independent reference and field-width choices to the tail grammar. · value `0` |
| 73 | 1 | `reference_prefix_payload` | `bytes[1]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 74 | 11 | `third_reference` | `bytes[11]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |
| 85 | 2 | `zero_tail` | `bytes[2]` | little | derived | Apply the independent reference and field-width choices to the tail grammar. |

## `assembly_alternate_rigid_group_prefix`

Spec §Assembly operands · layout: byte offsets · size: 67 B

The alternate ordinary prologue through its first two member references. Offsets are relative to the primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed |
| 19 | 1 | `leading_block_presence` | `u8` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed · value `0` |
| 20 | 1 | `property_block_presence` | `u8` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed · value `1` |
| 21 | 4 | `property_count` | `u32` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed · value `0` |
| 25 | 1 | `zero_separator` | `u8` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed · value `0` |
| 26 | 11 | `pathless_locator_reference` | `bytes[11]` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed |
| 37 | 4 | `zero_run_4` | `bytes[4]` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed |
| 41 | 4 | `locator_count` | `u32` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed |
| 45 | 11 | `first_locator_reference` | `bytes[11]` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed |
| 56 | 11 | `second_locator_reference` | `bytes[11]` | little | spec | An alternate ordinary prologue stores nine zero bytes after its indexed |

## `assembly_compact_rigid_group_identity_locator`

Spec §Assembly operands · layout: byte offsets · size: 52 B

Offsets are relative to the locator's indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The compact identity |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The compact identity |
| 21 | 1 | `construction_reference` | `bytes[1]` | little | spec | The compact identity |
| 22 | 1 | `identity_flag` | `u8` | little | spec | The compact identity |
| 23 | 1 | `separator` | `u8` | little | spec | The compact identity |
| 24 | 11 | `scope_backlink` | `bytes[11]` | little | spec | The compact identity |
| 35 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | The compact identity |
| 46 | 4 | `discriminator` | `u32` | little | spec | The compact identity |
| 50 | 2 | `zero_tail` | `bytes[2]` | little | spec | The compact identity |

## `assembly_ordinary_rigid_group_identity_locator`

Spec §Assembly operands · layout: byte offsets · size: 62 B

Offsets are relative to the locator's indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The ordinary identity |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The ordinary identity |
| 21 | 11 | `construction_reference` | `bytes[11]` | little | spec | The ordinary identity |
| 32 | 1 | `identity_flag` | `u8` | little | spec | The ordinary identity |
| 33 | 1 | `separator` | `u8` | little | spec | The ordinary identity |
| 34 | 11 | `scope_backlink` | `bytes[11]` | little | spec | The ordinary identity |
| 45 | 11 | `wrapper_reference` | `bytes[11]` | little | spec | The ordinary identity |
| 56 | 4 | `discriminator` | `u32` | little | spec | The ordinary identity |
| 60 | 2 | `zero_tail` | `bytes[2]` | little | spec | The ordinary identity |

## `assembly_null_typed_rigid_group_matrix_locator`

Spec §Assembly operands · layout: byte offsets · size: 259 B

Offsets are relative to the locator's indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The null-construction matrix form |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The null-construction matrix form |
| 21 | 1 | `construction_reference` | `bytes[1]` | little | spec | The null-construction matrix form |
| 22 | 1 | `identity_flag` | `u8` | little | spec | The null-construction matrix form |
| 23 | 128 | `transform` | `f64[16]` | little | spec | The null-construction matrix form |
| 151 | 51 | `scope_backlink` | `bytes[51]` | little | spec | The null-construction matrix form |
| 202 | 51 | `wrapper_reference` | `bytes[51]` | little | spec | The null-construction matrix form |
| 253 | 4 | `discriminator` | `u32` | little | spec | The null-construction matrix form |
| 257 | 2 | `zero_tail` | `bytes[2]` | little | spec | The null-construction matrix form |

## `assembly_null_typed_rigid_group_identity_locator`

Spec §Assembly operands · layout: byte offsets · size: 131 B

Offsets are relative to the locator's indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/assembly_operand_paths.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | The null-construction identity form |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The null-construction identity form |
| 21 | 1 | `construction_reference` | `bytes[1]` | little | spec | The null-construction identity form |
| 22 | 1 | `identity_flag` | `u8` | little | spec | The null-construction identity form |
| 23 | 51 | `scope_backlink` | `bytes[51]` | little | spec | The null-construction identity form |
| 74 | 51 | `wrapper_reference` | `bytes[51]` | little | spec | The null-construction identity form |
| 125 | 4 | `discriminator` | `u32` | little | spec | The null-construction identity form |
| 129 | 2 | `zero_tail` | `bytes[2]` | little | spec | The null-construction identity form |

## `legacy_attributed_external_placement_prefix`

Spec §1.4 · layout: byte offsets · size: 88 B

Offsets are relative to the primary indexed header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `class_tag_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `3` |
| 4 | 3 | `class_tag` | `bytes[3]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 7 | 8 | `entity` | `u64` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 15 | 4 | `instance_name_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 19 | 1 | `path_marker` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `1` |
| 20 | 4 | `path_count` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `1` |
| 24 | 60 | `target_reference` | `bytes[60]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 84 | 4 | `path_discriminator` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word |

## `legacy_attributed_external_placement_empty_attributes`

Spec §1.4 · layout: byte offsets · size: 5 B

Offsets are relative to the byte after the path discriminator.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `1` |
| 1 | 4 | `count` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |

## `legacy_attributed_external_placement_attributes`

Spec §1.4 · layout: byte offsets · size: 137 B

Offsets are relative to the byte after the path discriminator. The resolution value is the zero UTF-16 code-unit count.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `1` |
| 1 | 4 | `count` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `3` |
| 5 | 4 | `parent_name_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `13` |
| 9 | 13 | `parent_name` | `bytes[13]` | little | spec | An attributed legacy occurrence placement with a zero control word · value `"GatedByParent"` |
| 22 | 4 | `parent_type_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `21` |
| 26 | 21 | `parent_type` | `bytes[21]` | little | spec | An attributed legacy occurrence placement with a zero control word · value `"IntrinsicMetaTypebool"` |
| 47 | 1 | `parent_value` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 48 | 4 | `resolution_name_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `14` |
| 52 | 14 | `resolution_name` | `bytes[14]` | little | spec | An attributed legacy occurrence placement with a zero control word · value `"ResolutionTime"` |
| 66 | 4 | `resolution_type_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `24` |
| 70 | 24 | `resolution_type` | `bytes[24]` | little | spec | An attributed legacy occurrence placement with a zero control word · value `"IntrinsicMetaTypeHString"` |
| 94 | 4 | `resolution_value` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 98 | 4 | `self_name_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `9` |
| 102 | 9 | `self_name` | `bytes[9]` | little | spec | An attributed legacy occurrence placement with a zero control word · value `"SelfGated"` |
| 111 | 4 | `self_type_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `21` |
| 115 | 21 | `self_type` | `bytes[21]` | little | spec | An attributed legacy occurrence placement with a zero control word · value `"IntrinsicMetaTypebool"` |
| 136 | 1 | `self_value` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |

## `legacy_attributed_external_placement_members`

Spec §1.4 · layout: byte offsets · size: 235 B

Offsets are relative to the byte after the complete attribute block.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `control` | `u16` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 2 | 4 | `ordinal` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 6 | 4 | `metadata_guid_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `36` |
| 10 | 72 | `metadata_guid` | `bytes[72]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 82 | 4 | `tracking_guid_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `36` |
| 86 | 72 | `tracking_guid` | `bytes[72]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 158 | 15 | `metadata_reference` | `bytes[15]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 173 | 1 | `reference_separator` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 174 | 60 | `repeated_target_reference` | `bytes[60]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 234 | 1 | `identity_flag` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word |

## `legacy_attributed_external_placement_identity_tail`

Spec §1.4 · layout: byte offsets · size: 92 B

Offsets are relative to the byte after the identity flag.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `zero` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 4 | 4 | `role_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `36` |
| 8 | 72 | `role` | `bytes[72]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 80 | 1 | `separator` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 81 | 1 | `owner_marker` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `1` |
| 82 | 8 | `owner` | `u64` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 90 | 2 | `owner_tail` | `u16` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |

## `legacy_attributed_external_placement_matrix_tail`

Spec §1.4 · layout: byte offsets · size: 220 B

Offsets are relative to the byte after the identity flag.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 128 | `matrix` | `f64[16]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 128 | 4 | `zero` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 132 | 4 | `role_length` | `u32` | little | spec | An attributed legacy occurrence placement with a zero control word · value `36` |
| 136 | 72 | `role` | `bytes[72]` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 208 | 1 | `separator` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |
| 209 | 1 | `owner_marker` | `u8` | little | spec | An attributed legacy occurrence placement with a zero control word · value `1` |
| 210 | 8 | `owner` | `u64` | little | spec | An attributed legacy occurrence placement with a zero control word |
| 218 | 2 | `owner_tail` | `u16` | little | spec | An attributed legacy occurrence placement with a zero control word · value `0` |

## `edge_flange_corner_operation`

Spec §3.1 · layout: byte offsets · size: 620 B

Offsets are relative to the primary scope header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 101 | 4 | `bend_position` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `2` |
| 105 | 4 | `edge_count` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `2` |
| 109 | 11 | `edge_wrapper_one_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 120 | 11 | `edge_wrapper_two_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 131 | 11 | `settings_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 142 | 4 | `height_datum` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `2` |
| 146 | 11 | `angle_owner_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 157 | 11 | `height_owner_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 168 | 4 | `reference_side` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `4` |
| 172 | 11 | `side_padding` | `bytes[11]` | little | spec | The side padding, radius padding, and group padding are zero bytes · value `[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 183 | 8 | `inside_bend_radius` | `f64` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 191 | 6 | `radius_padding` | `bytes[6]` | little | spec | The side padding, radius padding, and group padding are zero bytes · value `[0, 0, 0, 0, 0, 0]` |
| 197 | 4 | `result_count` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `3` |
| 201 | 11 | `result_one_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 212 | 4 | `result_one_trailer` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `1` |
| 216 | 11 | `result_two_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 227 | 4 | `result_two_trailer` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `1` |
| 231 | 11 | `result_three_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 242 | 4 | `result_three_trailer` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `0` |
| 246 | 4 | `result_separator` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `1` |
| 250 | 11 | `aggregate_group_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 261 | 12 | `group_padding` | `bytes[12]` | little | spec | The side padding, radius padding, and group padding are zero bytes · value `[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 273 | 4 | `edge_group_count` | `u32` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` · value `2` |
| 277 | 11 | `edge_group_one_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |
| 288 | 11 | `edge_group_two_reference` | `bytes[11]` | little | spec | The fixed fields are defined by `edge_flange_corner_operation` |

Unstated regions:

- `0..101` (101 B): Scope envelope or uninterpreted bytes.
- `299..620` (321 B): Scope envelope follows the edge groups.

## `operation_table_header`

Spec §1.4 · layout: byte offsets · size: 15 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/xref/operation_pose.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `class_tag_length` | `u32` | little | spec | a three-digit LP-ASCII class tag · value `3` |
| 4 | 3 | `class_tag` | `bytes[3]` | little | spec | a three-digit LP-ASCII class tag |
| 7 | 8 | `record_index` | `u64` | little | spec | a u64 indexed record identity |

## `current_extrude_face_target_payload`

Spec §3.1 · layout: byte offsets · size: 54 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/fixed_parameters.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `first_side_extent` | `u32` | little | spec | The current face-target payload starts at the first-side extent |
| 4 | 11 | `first_side_offset_reference` | `bytes[11]` | little | spec | first-side offset reference at payload offset 4 |
| 19 | 4 | `target_option` | `u32` | little | spec | target option at offset 19 is one · value `1` |
| 23 | 4 | `second_side_extent` | `u32` | little | spec | offset 23 stores the second-side extent |
| 28 | 11 | `second_side_offset_reference` | `bytes[11]` | little | spec | offset 28 stores the second-side offset reference |
| 43 | 11 | `second_side_taper_reference` | `bytes[11]` | little | spec | offset 43 stores the second-side taper reference |

Unstated regions:

- `15..19` (4 B): Target padding.
- `27..28` (1 B): Target separator.
- `39..43` (4 B): Target padding.

## `emboss_single_control_head`

Spec §3.1 · layout: byte offsets · size: 61 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/emboss.rs`
- `crates/cadmpeg-codec-f3d/src/records/feature/emboss.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 11 | 9 | `zero_run` | `bytes[9]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane · value `[0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 20 | 4 | `control_words` | `u32` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 24 | 11 | `depth_owner_reference` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 35 | 11 | `auxiliary_group_reference` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 46 | 11 | `horizontal_owner_reference` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 57 | 4 | `native_pair_count` | `u32` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |

## `emboss_two_control_head`

Spec §3.1 · layout: byte offsets · size: 65 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/emboss.rs`
- `crates/cadmpeg-codec-f3d/src/records/feature/emboss.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 11 | 9 | `zero_run` | `bytes[9]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane · value `[0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 20 | 8 | `control_words` | `u32[2]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 28 | 11 | `depth_owner_reference` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 39 | 11 | `auxiliary_group_reference` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 50 | 11 | `horizontal_owner_reference` | `bytes[11]` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |
| 61 | 4 | `native_pair_count` | `u32` | little | spec | Both heads contain the indexed header, nine zero bytes, the control lane |

## `emboss_native_pair`

Spec §3.1 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/emboss.rs`
- `crates/cadmpeg-codec-f3d/src/records/feature/emboss.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `values` | `u64[2]` | little | spec | An `Emboss` native pair contains two u64 values. |

## `emboss_counted_suffix`

Spec §3.1 · layout: byte offsets · size: 158 B

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/scopes/emboss.rs`
- `crates/cadmpeg-codec-f3d/src/records/feature/emboss.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `rotation_owner_reference` | `bytes[11]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. |
| 11 | 11 | `face_group_reference` | `bytes[11]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. |
| 22 | 11 | `vertical_owner_reference` | `bytes[11]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. |
| 33 | 4 | `profile_group_count` | `u32` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. · value `1` |
| 37 | 11 | `profile_group_reference` | `bytes[11]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. |
| 48 | 12 | `zero_tail` | `bytes[12]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. · value `[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 60 | 4 | `face_group_count` | `u32` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. · value `1` |
| 64 | 11 | `repeated_face_group_reference` | `bytes[11]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. |
| 75 | 4 | `null_guid_count` | `u32` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. · value `36` |
| 79 | 72 | `null_guid` | `u16[36]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. |
| 151 | 3 | `zero_separator` | `bytes[3]` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. · value `[0, 0, 0]` |
| 154 | 4 | `reference_count` | `u32` | little | spec | The `emboss_counted_suffix` layout starts immediately after the native pair run. |

## `sketch_typed_compact_member_head`

Spec §3.1 · layout: byte offsets · size: 74 B

Offsets are relative to the indexed head-placement header.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/sketch.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 10 | `zero_run` | `bytes[10]` | little | spec | It stores ten zero bytes after the indexed header · value `[0, 0, 0, 0, 0, 0, 0, 0, 0, 0]` |
| 21 | 2 | `flags` | `bytes[2]` | little | spec | `01 00` at offsets 21 and 22 · value `[1, 0]` |
| 23 | 51 | `reference` | `bytes[51]` | little | spec | a 51-byte typed reference at offset 23 |

Unstated regions:

- `0..11` (11 B): The indexed header precedes the placement fields.

## `derived_base_feature_scope_711`

Spec §3.1 · layout: byte offsets · size: 711 B

Offsets are relative to the primary indexed header of a `Derived Base Feature` scope with one result body. Element tails are not tabulated; the trailing member fields of the scope reference and parameter elements are listed.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 1 | `body_marker` | `u8` | little | spec | The u32 at offset 20 of a derived base frame with result bodies is twice the body count · value `1` |
| 20 | 4 | `body_count_twice` | `u32` | little | spec | The u32 at offset 20 of a derived base frame with result bodies is twice the body count · value `2` |
| 24 | 1 | `body_first_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 25 | 8 | `body_first_record` | `u64` | little | spec | In the 711-byte form the second body element stores the Design body entity suffix |
| 33 | 4 | `body_first_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 37 | 36 | `body_first_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D3937028-C20C-4E65-B010-94AD418A5C20"` |
| 79 | 1 | `body_entity_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 80 | 8 | `body_entity_record` | `u64` | little | spec | In the 711-byte form the second body element stores the Design body entity suffix |
| 88 | 4 | `body_entity_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 92 | 36 | `body_entity_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D3937028-C20C-4E65-B010-94AD418A5C20"` |
| 167 | 1 | `body_first_repeat_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 168 | 8 | `body_first_repeat_record` | `u64` | little | spec | In the 711-byte form the second body element stores the Design body entity suffix |
| 176 | 4 | `body_first_repeat_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 180 | 36 | `body_first_repeat_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D3937028-C20C-4E65-B010-94AD418A5C20"` |
| 219 | 1 | `scope_ref_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 220 | 8 | `scope_ref_record` | `u64` | little | spec | The scope reference elements store the primary record index plus 4 |
| 228 | 4 | `scope_ref_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 232 | 36 | `scope_ref_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"493A89BA-E146-498D-9033-2783FE727AF4"` |
| 274 | 1 | `other_ref_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 275 | 8 | `other_ref_record` | `u64` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID |
| 283 | 4 | `other_ref_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 287 | 36 | `other_ref_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D26351F0-5940-4D23-AA20-2C35475A6D9E"` |
| 333 | 4 | `zero_role_length` | `u32` | little | spec | A 36-code-unit LP-UTF16 GUID of zeros precedes the second scope reference element · value `36` |
| 337 | 72 | `zero_role` | `bytes[72]` | little | spec | A 36-code-unit LP-UTF16 GUID of zeros precedes the second scope reference element |
| 416 | 1 | `scope_ref_repeat_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 417 | 8 | `scope_ref_repeat_record` | `u64` | little | spec | The scope reference elements store the primary record index plus 4 |
| 425 | 4 | `scope_ref_repeat_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 429 | 36 | `scope_ref_repeat_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"493A89BA-E146-498D-9033-2783FE727AF4"` |
| 467 | 4 | `current_history_state_id` | `u32` | little | spec | The trailing member of the second scope reference element is the current history-state identity |
| 471 | 4 | `kind_length` | `u32` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal · value `20` |
| 475 | 40 | `kind` | `bytes[40]` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal |
| 515 | 4 | `feature_ordinal` | `u32` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal |
| 534 | 1 | `param_3_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 535 | 8 | `param_3_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 543 | 4 | `param_3_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 547 | 36 | `param_3_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"B7F34D7B-DAC4-41A5-B67D-D672537C3BFA"` |
| 585 | 4 | `previous_history_state_id` | `u32` | little | spec | The trailing member of the first trailing parameter element is the previous history-state identity |
| 589 | 4 | `scalar_head` | `u32` | little | spec | An f64 between two equal u32 values follows the previous history-state identity · value `76` |
| 593 | 8 | `scalar` | `f64` | little | spec | An f64 between two equal u32 values follows the previous history-state identity |
| 601 | 4 | `scalar_tail` | `u32` | little | spec | An f64 between two equal u32 values follows the previous history-state identity · value `76` |
| 605 | 1 | `param_2_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 606 | 8 | `param_2_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 614 | 4 | `param_2_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 618 | 36 | `param_2_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"5FBE538E-139B-46D4-BCAF-CC796D1E1F78"` |
| 658 | 1 | `param_1_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 659 | 8 | `param_1_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 667 | 4 | `param_1_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 671 | 36 | `param_1_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"6CCF41D5-40BE-48ED-A834-18F3EAED6C57"` |

Unstated regions:

- `0..19` (19 B): The specification states no field in this range.
- `73..79` (6 B): The specification states no field in this range.
- `128..167` (39 B): The specification states no field in this range.
- `216..219` (3 B): The specification states no field in this range.
- `268..274` (6 B): The specification states no field in this range.
- `323..333` (10 B): The specification states no field in this range.
- `409..416` (7 B): The specification states no field in this range.
- `465..467` (2 B): The specification states no field in this range.
- `519..534` (15 B): The specification states no field in this range.
- `583..585` (2 B): The specification states no field in this range.
- `654..658` (4 B): The specification states no field in this range.
- `707..711` (4 B): The specification states no field in this range.

## `derived_base_feature_scope_707`

Spec §3.1 · layout: byte offsets · size: 707 B

Offsets are relative to the primary indexed header of a `DerivedBaseFeature` scope with one result body.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 19 | 1 | `body_marker` | `u8` | little | spec | The u32 at offset 20 of a derived base frame with result bodies is twice the body count · value `1` |
| 20 | 4 | `body_count_twice` | `u32` | little | spec | The u32 at offset 20 of a derived base frame with result bodies is twice the body count · value `2` |
| 24 | 1 | `body_first_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 25 | 8 | `body_first_record` | `u64` | little | spec | In the 711-byte form the second body element stores the Design body entity suffix |
| 33 | 4 | `body_first_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 37 | 36 | `body_first_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D3937028-C20C-4E65-B010-94AD418A5C20"` |
| 79 | 1 | `body_entity_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 80 | 8 | `body_entity_record` | `u64` | little | spec | In the 711-byte form the second body element stores the Design body entity suffix |
| 88 | 4 | `body_entity_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 92 | 36 | `body_entity_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D3937028-C20C-4E65-B010-94AD418A5C20"` |
| 167 | 1 | `body_first_repeat_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 168 | 8 | `body_first_repeat_record` | `u64` | little | spec | In the 711-byte form the second body element stores the Design body entity suffix |
| 176 | 4 | `body_first_repeat_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 180 | 36 | `body_first_repeat_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D3937028-C20C-4E65-B010-94AD418A5C20"` |
| 219 | 1 | `scope_ref_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 220 | 8 | `scope_ref_record` | `u64` | little | spec | The scope reference elements store the primary record index plus 4 |
| 228 | 4 | `scope_ref_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 232 | 36 | `scope_ref_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"493A89BA-E146-498D-9033-2783FE727AF4"` |
| 274 | 1 | `other_ref_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 275 | 8 | `other_ref_record` | `u64` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID |
| 283 | 4 | `other_ref_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 287 | 36 | `other_ref_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"D26351F0-5940-4D23-AA20-2C35475A6D9E"` |
| 333 | 4 | `zero_role_length` | `u32` | little | spec | A 36-code-unit LP-UTF16 GUID of zeros precedes the second scope reference element · value `36` |
| 337 | 72 | `zero_role` | `bytes[72]` | little | spec | A 36-code-unit LP-UTF16 GUID of zeros precedes the second scope reference element |
| 416 | 1 | `scope_ref_repeat_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 417 | 8 | `scope_ref_repeat_record` | `u64` | little | spec | The scope reference elements store the primary record index plus 4 |
| 425 | 4 | `scope_ref_repeat_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 429 | 36 | `scope_ref_repeat_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"493A89BA-E146-498D-9033-2783FE727AF4"` |
| 467 | 4 | `current_history_state_id` | `u32` | little | spec | The trailing member of the second scope reference element is the current history-state identity |
| 471 | 4 | `kind_length` | `u32` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal · value `18` |
| 475 | 36 | `kind` | `bytes[36]` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal |
| 511 | 4 | `feature_ordinal` | `u32` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal |
| 530 | 1 | `param_3_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 531 | 8 | `param_3_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 539 | 4 | `param_3_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 543 | 36 | `param_3_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"B7F34D7B-DAC4-41A5-B67D-D672537C3BFA"` |
| 581 | 4 | `previous_history_state_id` | `u32` | little | spec | The trailing member of the first trailing parameter element is the previous history-state identity |
| 585 | 4 | `scalar_head` | `u32` | little | spec | An f64 between two equal u32 values follows the previous history-state identity · value `69` |
| 589 | 8 | `scalar` | `f64` | little | spec | An f64 between two equal u32 values follows the previous history-state identity |
| 597 | 4 | `scalar_tail` | `u32` | little | spec | An f64 between two equal u32 values follows the previous history-state identity · value `69` |
| 601 | 1 | `param_2_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 602 | 8 | `param_2_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 610 | 4 | `param_2_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 614 | 36 | `param_2_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"5FBE538E-139B-46D4-BCAF-CC796D1E1F78"` |
| 654 | 1 | `param_1_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 655 | 8 | `param_1_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 663 | 4 | `param_1_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 667 | 36 | `param_1_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"6CCF41D5-40BE-48ED-A834-18F3EAED6C57"` |

Unstated regions:

- `0..19` (19 B): The specification states no field in this range.
- `73..79` (6 B): The specification states no field in this range.
- `128..167` (39 B): The specification states no field in this range.
- `216..219` (3 B): The specification states no field in this range.
- `268..274` (6 B): The specification states no field in this range.
- `323..333` (10 B): The specification states no field in this range.
- `409..416` (7 B): The specification states no field in this range.
- `465..467` (2 B): The specification states no field in this range.
- `515..530` (15 B): The specification states no field in this range.
- `579..581` (2 B): The specification states no field in this range.
- `650..654` (4 B): The specification states no field in this range.
- `703..707` (4 B): The specification states no field in this range.

## `derived_base_feature_scope_491`

Spec §3.1 · layout: byte offsets · size: 491 B

Offsets are relative to the primary indexed header of a `DerivedBaseFeature` zero-body scope.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 20 | 1 | `zero_body_marker` | `u8` | little | spec | A zero-body derived base frame stores `u8 1` at offset 20 · value `1` |
| 54 | 1 | `scope_ref_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 55 | 8 | `scope_ref_record` | `u64` | little | spec | The scope reference elements store the primary record index plus 4 |
| 63 | 4 | `scope_ref_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 67 | 36 | `scope_ref_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"493A89BA-E146-498D-9033-2783FE727AF4"` |
| 117 | 4 | `zero_role_length` | `u32` | little | spec | A 36-code-unit LP-UTF16 GUID of zeros precedes the second scope reference element · value `36` |
| 121 | 72 | `zero_role` | `bytes[72]` | little | spec | A 36-code-unit LP-UTF16 GUID of zeros precedes the second scope reference element |
| 200 | 1 | `scope_ref_repeat_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 201 | 8 | `scope_ref_repeat_record` | `u64` | little | spec | The scope reference elements store the primary record index plus 4 |
| 209 | 4 | `scope_ref_repeat_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 213 | 36 | `scope_ref_repeat_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"493A89BA-E146-498D-9033-2783FE727AF4"` |
| 251 | 4 | `current_history_state_id` | `u32` | little | spec | The trailing member of the second scope reference element is the current history-state identity |
| 255 | 4 | `kind_length` | `u32` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal · value `18` |
| 259 | 36 | `kind` | `bytes[36]` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal |
| 295 | 4 | `feature_ordinal` | `u32` | little | spec | The kind is length-prefixed UTF-16, followed by the feature ordinal |
| 314 | 1 | `param_3_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 315 | 8 | `param_3_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 323 | 4 | `param_3_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 327 | 36 | `param_3_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"B7F34D7B-DAC4-41A5-B67D-D672537C3BFA"` |
| 365 | 4 | `previous_history_state_id` | `u32` | little | spec | The trailing member of the first trailing parameter element is the previous history-state identity |
| 369 | 4 | `scalar_head` | `u32` | little | spec | An f64 between two equal u32 values follows the previous history-state identity · value `69` |
| 373 | 8 | `scalar` | `f64` | little | spec | An f64 between two equal u32 values follows the previous history-state identity |
| 381 | 4 | `scalar_tail` | `u32` | little | spec | An f64 between two equal u32 values follows the previous history-state identity · value `69` |
| 385 | 1 | `param_2_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 386 | 8 | `param_2_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 394 | 4 | `param_2_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 398 | 36 | `param_2_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"5FBE538E-139B-46D4-BCAF-CC796D1E1F78"` |
| 438 | 1 | `param_1_marker` | `u8` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `1` |
| 439 | 8 | `param_1_record` | `u64` | little | spec | The three trailing parameter elements store the primary record index plus 3, 2, and 1 |
| 447 | 4 | `param_1_role_length` | `u32` | little | spec | A typed-reference element of a derived base frame is `u8 1`, a u64 Design record index, u32 36, and a 36-byte uppercase ASCII role GUID · value `36` |
| 451 | 36 | `param_1_role` | `bytes[36]` | little | spec | Equal roles have equal GUIDs in different documents · value `"6CCF41D5-40BE-48ED-A834-18F3EAED6C57"` |

Unstated regions:

- `0..20` (20 B): The specification states no field in this range.
- `21..54` (33 B): The specification states no field in this range.
- `103..117` (14 B): The specification states no field in this range.
- `193..200` (7 B): The specification states no field in this range.
- `249..251` (2 B): The specification states no field in this range.
- `299..314` (15 B): The specification states no field in this range.
- `363..365` (2 B): The specification states no field in this range.
- `434..438` (4 B): The specification states no field in this range.
- `487..491` (4 B): The specification states no field in this range.

## `entity_tracking_carrier_prefix`

Spec §3.1 · layout: byte offsets · size: 73 B

Offsets are relative to the carrier indexed header; the optional related identities follow at offset 73.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/operands.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 10 | `zero_run_10` | `bytes[10]` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 21 | 12 | `lead_words` | `bytes[12]` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 33 | 4 | `lane` | `u32` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 37 | 8 | `primary_identity` | `u64` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 45 | 8 | `zero_run_8_first` | `u64` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 53 | 4 | `one` | `u32` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 57 | 4 | `selector` | `i32` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 61 | 4 | `kind` | `u32` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |
| 65 | 8 | `zero_run_8_second` | `u64` | little | spec | The carrier header is followed by ten zero bytes, u32 values `1, 0, 1`, a u32 lane with value `2` or `3` |

Unstated regions:

- `0..11` (11 B): The indexed header precedes this field run.

## `local_component_occurrence_empty_attributes_prefix`

Spec §3.1 · layout: byte offsets · size: 213 B

Offsets are relative to the indexed header. The identity or matrix tail follows at offset 213.

Parsed by:
- `crates/cadmpeg-codec-f3d/src/design/decode/components.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 11 | 8 | `empty_instance_name` | `bytes[8]` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |
| 19 | 1 | `path_marker` | `u8` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. · value `1` |
| 20 | 4 | `path_count` | `u32` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. · value `1` |
| 24 | 9 | `component_reference` | `bytes[9]` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |
| 35 | 4 | `path_discriminator` | `u32` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |
| 39 | 1 | `attributes_marker` | `u8` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. · value `1` |
| 40 | 4 | `attribute_count` | `u32` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. · value `0` |
| 44 | 4 | `occurrence_ordinal` | `u32` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |
| 48 | 76 | `carrier_guid` | `bytes[76]` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |
| 124 | 76 | `occurrence_guid` | `bytes[76]` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |
| 201 | 9 | `component_reference_repeat` | `bytes[9]` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |
| 212 | 1 | `identity_flag` | `u8` | little | spec | An empty-attribute local component occurrence stores an empty attribute list after its path. |

Unstated regions:

- `0..11` (11 B): Indexed header.
- `33..35` (2 B): Two zero trailing bytes of the local component reference.
- `200..201` (1 B): Zero separator.
- `210..212` (2 B): Zero trailing field of the repeated component-record reference.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Container, manifest, and stream-selection layers (§1, §2) | §1.3 | ZIP entries and `Manifest.dat` text grammar; no fixed byte-offset structure is stated. |
