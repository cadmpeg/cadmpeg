<!-- Generated from docs/layouts/f3d.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `f3d` record layouts

Source of truth: [`docs/formats/f3d.md`](../../docs/formats/f3d.md).
Table source: `docs/layouts/f3d.toml`.

Covers the fixed Design-segment headers, parameter-owner prefix, and body-map prefix, the named solid-primitive prologue,
the ParaMesh entry-name, container-GUID, body graph, collection, texture table,
feature scope, current and shifted Extrude operation and extent sections,
wrapper, and Scene records,
the compact and ten-reference `CoilPrimitive` prologues and matrix blocks, the
compact `Loft` prefix and nested profile-region frames, the class-418
`SplitFace` prefix, the grouped recipe-reference prefix, the three `Combine`
operation prologues and cross-document selector, the axial `Assemble` carrier
and selector prefixes, the non-axial assembly-operation operand-path locator run,
locator, and wrapper, and the sheet-metal `EdgeFlange` fixed operation section
(§3.1), plus the `Decal` scope, image-record prefixes, and current sketch-container visibility member. ASM stream records are tabulated in `docs/layouts/asm.toml`. Protein page records are tabulated in `docs/layouts/protein.toml`.
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

## `base_feature_result_body_prefix`

Spec §3.1 · layout: byte offsets · size: 24 B

Offsets are relative to the primary indexed header. The two parallel 15-byte body-entry runs begin at offset 24.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 11 | 8 | `zero_run_8` | `bytes[8]` | little | spec | bytes 11 through 18 are zero |
| 19 | 1 | `body_count_marker` | `u8` | little | spec | byte 19 is `0x01` |
| 20 | 4 | `combined_body_reference_count` | `u32` | little | spec | offset 20 stores u32 `2N` |

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

Offsets are relative to the byte after the two parallel 15-byte body-entry runs. The class-420 and class-452 compact forms use this field.

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

## `split_face_class_418_prefix`

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

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| Container, manifest, and stream-selection layers (§1, §2) | §1.3 | ZIP entries and `Manifest.dat` text grammar; no fixed byte-offset structure is stated. |
