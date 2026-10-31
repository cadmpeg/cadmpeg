<!-- Generated from docs/layouts/inventor.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `inventor` record layouts

Source of truth: [`docs/formats/inventor.md`](../../docs/formats/inventor.md).
Table source: `docs/layouts/inventor.toml`.

Fixed prefixes and descriptors in the supported RSe schema-31, Meta Stream v8, Protein stream envelope, UFRxDoc, and kernel-carrier envelope. The Protein page format is tabulated in `docs/layouts/protein.toml`.

## `pm_dc_derived_source_field`

Spec §13.4 · layout: byte offsets · size: 4 B

Offset starts after the segment-major-19 content header.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/derived.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `component` | `u32` | little | spec | one qualified component reference |

## `pm_dc_derived_component_fields`

Spec §13.4 · layout: byte offsets · size: 32 B

Offsets start after the segment-major-19 state content header.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/derived.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `i32` | little | spec | state i32 = -1 · value `-1` |
| 4 | 16 | `references` | `u32[4]` | little | spec | references u32[4] |
| 20 | 4 | `marker` | `u32` | little | spec | marker u32 = 0x30000004 · value `805306372` |
| 24 | 4 | `count` | `u32` | little | spec | count u32 = 0 · value `0` |
| 28 | 4 | `terminal` | `u32` | little | spec | terminal u32 |

## `pm_dc_content_header`

Spec §12 · layout: byte offsets · size: 22 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |
| 10 | 4 | `flags` | `u32` | little | spec | flags u32 |
| 14 | 4 | `context_reference` | `u32` | little | spec | context_reference u32 |
| 18 | 4 | `source_index` | `u32` | little | spec | source_index u32 |

## `pm_dc_content_header_v28`

Spec §12 · layout: byte offsets · size: 26 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |
| 10 | 4 | `flags` | `u32` | little | spec | flags u32 |
| 14 | 4 | `flags_extension` | `u32` | little | spec | flags_extension u32 = 0 |
| 18 | 4 | `context_reference` | `u32` | little | spec | context_reference u32 |
| 22 | 4 | `source_index` | `u32` | little | spec | source_index u32 |

## `pm_dc_sketch_entity_header`

Spec §12 · layout: byte offsets · size: 30 B

Offsets hold in segment major versions 15 through 22. Later versions extend the embedded content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | after the content header |
| 22 | 4 | `entity_flags` | `u32` | little | spec | entity_flags:u32 |
| 26 | 4 | `sketch_reference` | `u32` | little | spec | sketch_reference:u32 |

## `pm_dc_reference_list_prefix`

Spec §12 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `u16[2]` | little | spec | u16 values `2, 0x3000` |
| 4 | 4 | `item_count` | `u32` | little | spec | and a u32 count |

## `pm_dc_reference_array_prefix`

Spec §12 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `u16[2]` | little | spec | u16 values `8, 0x3000` |
| 4 | 4 | `item_count` | `u32` | little | spec | and a u32 count |

## `pm_dc_constraint_prefix`

Spec §12 · layout: byte offsets · size: 30 B

Offsets hold in segment major versions 15 through 22. Later versions extend the embedded content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | the content header |
| 22 | 4 | `state` | `i32` | little | spec | state:i32 |
| 26 | 4 | `group_reference` | `u32` | little | spec | a group reference |

## `pm_dc_constraint_v15_v16_header`

Spec §12 · layout: byte offsets · size: 34 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 30 | `constraint_prefix` | `bytes[30]` | little | spec | starts with the content header, `state:i32`, and a group reference |
| 30 | 4 | `parameter_reference` | `u32` | little | spec | the parameter reference immediately after the group reference |

## `pm_dc_constraint_map_prefix`

Spec §12 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `u16[2]` | little | spec | u16 values `6, 0x3000` |
| 4 | 4 | `item_count` | `u32` | little | spec | and a u32 count |

## `pm_dc_feature_prefix`

Spec §13 · layout: byte offsets · size: 38 B

Offsets hold in segment major versions 15 through 22. Later versions extend the embedded content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | the 22-byte content header |
| 22 | 4 | `state` | `i32` | little | spec | state:i32 |
| 26 | 4 | `outline_value` | `u32` | little | spec | outline_value:u32 |
| 30 | 8 | `property_list_prefix` | `bytes[8]` | little | spec | a type-2 property-reference list prefix |

## `pm_dc_feature_terminator`

Spec §13 · layout: byte offsets · size: 26 B

Offsets hold in segment major versions 15 through 22. Later versions extend the embedded content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | the 22-byte content header |
| 22 | 4 | `state` | `i32` | little | spec | followed by `state:i32` |

## `pm_dc_feature_enumeration`

Spec §13 · layout: byte offsets · size: 26 B

Offsets hold in segment major versions 15 through 22. Later versions extend the embedded content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | same 22-byte content header |
| 22 | 2 | `type_value` | `i16` | little | spec | type_value:i16 |
| 24 | 2 | `value` | `u16` | little | spec | value:u16 |

## `pm_dc_split_boolean_body`

Spec §13 · layout: byte offsets · size: 9 B

Offsets are relative to the body after the versioned PmDc content header and its stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/split.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `name_count` | `u32` | little | spec | name_count:u32 = 0 · value `0` |
| 4 | 4 | `name_value` | `u32` | little | spec | name_value:u32 = 0 · value `0` |
| 8 | 1 | `value` | `bool8` | little | spec | value:u8 |

## `pm_dc_chamfer_enumeration`

Spec §13 · layout: byte offsets · size: 30 B

Offsets hold in segment major versions 15 through 22. Later versions extend the embedded content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | It adds the same `type_value:i16` |
| 22 | 2 | `type_value` | `i16` | little | spec | type_value:i16 |
| 24 | 2 | `value` | `u16` | little | spec | value:u16 |
| 26 | 4 | `terminal_value` | `u32` | little | spec | followed by a zero u32 terminal value |

## `pm_dc_fillet_edge_selection`

Spec §13 · layout: byte offsets · size: 30 B

Offsets hold in segment major versions 15 through 22. Later versions extend the embedded content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | after the content header |
| 22 | 4 | `type_value` | `u32` | little | spec | type_value:u32 |
| 26 | 4 | `value` | `u32` | little | spec | value:u32 |

## `pm_dc_linked_element_header`

Spec §13 · layout: byte offsets · size: 26 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 8 | `values` | `u32[2]` | little | spec | values u32[2] |
| 14 | 4 | `owner_reference` | `u32` | little | spec | owner_reference u32 |
| 18 | 4 | `parent_reference` | `u32` | little | spec | parent_reference u32 |
| 22 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |

## `pm_dc_linked_element_header_v28`

Spec §13 · layout: byte offsets · size: 30 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 8 | `values` | `u32[2]` | little | spec | values u32[2] |
| 14 | 4 | `flags_extension` | `u32` | little | spec | flags_extension u32 = 0 |
| 18 | 4 | `owner_reference` | `u32` | little | spec | owner_reference u32 |
| 22 | 4 | `parent_reference` | `u32` | little | spec | parent_reference u32 |
| 26 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |

## `pm_dc_parameter_prefix`

Spec §11 · layout: byte offsets · size: 26 B

In segment major versions 15 through 22 the counted UTF-16LE parameter name starts at byte 22. Later versions extend the content header as section 12 states.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |
| 10 | 4 | `flags` | `u32` | little | spec | flags u32 |
| 14 | 4 | `context_reference` | `u32` | little | spec | context_reference u32 |
| 18 | 4 | `source_index` | `u32` | little | spec | source_index u32 |
| 22 | 4 | `name_code_units` | `u32` | little | spec | name counted UTF-16LE |

## `pm_dc_expression_header`

Spec §11 · layout: byte offsets · size: 10 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value:u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id:u16 |
| 6 | 4 | `unit_reference` | `u32` | little | spec | unit_reference:u32 |

## `pm_dc_unit_array_prefix`

Spec §11 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `u16[2]` | little | spec | u16 values `3, 0x3000` |
| 4 | 4 | `item_count` | `u32` | little | spec | a u32 count |

## `pm_dc_base_unit`

Spec §11 · layout: byte offsets · size: 22 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value:u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id:u16 |
| 6 | 8 | `magnitude` | `f64` | little | spec | magnitude:f64 |
| 14 | 8 | `factor` | `f64` | little | spec | factor:f64 |

## `rse_database_prefix`

Spec §2 · layout: byte offsets · size: 56 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `database_id` | `bytes[16]` | little | spec | 16-byte database identifier |
| 16 | 4 | `schema` | `u32` | little | spec | a u32 schema |
| 20 | 8 | `created_by` | `bytes[8]` | little | spec | an 8-byte creation-version tuple |
| 28 | 8 | `created_filetime` | `u64` | little | spec | a u64 creation FILETIME |
| 36 | 8 | `saved_by` | `bytes[8]` | little | spec | an 8-byte save-version tuple |
| 44 | 8 | `saved_filetime` | `u64` | little | spec | a u64 save FILETIME |
| 52 | 4 | `note_code_units` | `u32` | little | spec | a u32-counted UTF-16LE note |

## `bulk_envelope`

Spec §4 · layout: byte offsets · size: 18 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/rse.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `prefix` | `bytes[16]` | little | spec | a 16-byte prefix |
| 16 | 1 | `form` | `u8` | little | spec | Low byte of the retained u16 form. |
| 17 | 1 | `compression_method` | `u8` | little | spec | High byte of the retained u16 form: 1 = zlib; 2 = Zstandard. |

## `meta_body_prefix`

Spec §3 · layout: byte offsets · size: 14 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 14 | `values` | `u16[7]` | little | spec | seven u16 values |

## `meta_type_descriptor`

Spec §3 · layout: byte offsets · size: 28 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `type_id` | `bytes[16]` | little | spec | a 16-byte type identifier |
| 16 | 2 | `field_0_kind` | `u16` | little | spec | two `(u16, u32)` field pairs |
| 18 | 4 | `field_0_value` | `u32` | little | spec | two `(u16, u32)` field pairs |
| 22 | 2 | `field_1_kind` | `u16` | little | spec | two `(u16, u32)` field pairs |
| 24 | 4 | `field_1_value` | `u32` | little | spec | two `(u16, u32)` field pairs |

## `kernel_carrier_header`

Spec §5 · layout: byte offsets · size: 14 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_state` | `u32` | little | spec | u32 header state |
| 4 | 2 | `header_kind` | `u16` | little | spec | u16 header kind |
| 6 | 4 | `header_value` | `u32` | little | spec | u32 header value |
| 10 | 4 | `schema` | `u32` | little | spec | u32 schema |

## `protein_header`

Spec §7 · layout: byte offsets · size: 4 B

Inventor compound-stream envelope around a Protein ZIP. The Protein page format is tabulated in `docs/layouts/protein.toml`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `payload_len` | `u32` | little | spec | a u32 payload length |

## `ufrx_header`

Spec §8 · layout: byte offsets · size: 4 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `schema` | `u16` | little | spec | a u16 schema |
| 2 | 2 | `section_version_count` | `u16` | little | spec | a u16 section-version count |

## `pm_app_default_style_current`

Spec §10 · layout: byte offsets · size: 55 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `material_reference` | `u32` | little | spec | material_reference u32 |
| 10 | 4 | `rendering_style_reference` | `u32` | little | spec | rendering_style_reference u32 |
| 14 | 28 | `related_references` | `u32[7]` | little | spec | related_references u32[7] |
| 42 | 1 | `state` | `u8` | little | spec | state u8 |
| 43 | 4 | `terminal_reference` | `u32` | little | spec | terminal_reference u32 |
| 47 | 8 | `padding` | `bytes[8]` | little | spec | padding bytes[8] = 0 |

## `pm_app_rendering_style_current_prefix`

Spec §10 · layout: byte offsets · size: 27 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 1 | `state` | `u8` | little | spec | state u8 |
| 7 | 2 | `flags` | `u16` | little | spec | flags u16 |
| 9 | 2 | `padding` | `u16` | little | spec | padding u16 = 0 |
| 11 | 4 | `values` | `u16[2]` | little | spec | values u16[2] |
| 15 | 4 | `default_state` | `u32` | little | spec | default_state u32 |
| 19 | 4 | `value` | `u32` | little | spec | value u32 |
| 23 | 4 | `name_reference` | `u32` | little | spec | name_reference u32 |

## `pm_graphics_face_current_prefix`

Spec §10 · layout: byte offsets · size: 26 B

The variable edge-reference list starts at byte 26. Its exact end selects the fixed visibility, bounds, key, and values tail.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `flags` | `u32` | little | spec | flags u32 |
| 10 | 4 | `styles_reference` | `u32` | little | spec | styles_reference u32 |
| 14 | 4 | `surface_reference` | `u32` | little | spec | surface_reference u32 |
| 18 | 4 | `parent_reference` | `u32` | little | spec | parent_reference u32 |
| 22 | 4 | `state` | `u32` | little | spec | state u32 |

## `pm_graphics_list_prefix`

Spec §10 · layout: byte offsets · size: 8 B

A nonempty list continues with two u32 metadata values and its counted items.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `u16[2]` | little | spec | u16 values `2, 0x3000` |
| 4 | 4 | `item_count` | `u32` | little | spec | a u32 item count |

## `pm_graphics_primary_color_style_current`

Spec §10 · layout: byte offsets · size: 94 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 14 | `controls` | `u16[7]` | little | spec | controls u16[7] |
| 18 | 2 | `color_header` | `u8[2]` | little | spec | color_header u8[2] |
| 20 | 64 | `colors` | `f32[16]` | little | spec | Four consecutive RGBA vectors. |
| 84 | 4 | `color_tail` | `u16[2]` | little | spec | color_tail u16[2] |
| 88 | 1 | `state` | `u8` | little | spec | state u8 |
| 89 | 4 | `values` | `u16[2]` | little | spec | values u16[2] |
| 93 | 1 | `terminal_state` | `u8` | little | spec | terminal_state u8 |

## `ufrx_occurrence_prefix`

Spec §8 · layout: byte offsets · size: 20 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `reference_id` | `u32` | little | spec | reference_id u32 |
| 4 | 4 | `occurrence_id` | `u32` | little | spec | occurrence_id u32 |
| 8 | 4 | `occurrence_key` | `u32` | little | spec | occurrence_key u32 |
| 12 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 16 | 4 | `title_count` | `u32` | little | spec | title_count u32 |

## `assembly_occurrence_prefix`

Spec §9 · layout: byte offsets · size: 50 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |
| 10 | 4 | `flags` | `u32` | little | spec | flags u32 |
| 14 | 4 | `owner_reference` | `u32` | little | spec | owner_reference u32 |
| 18 | 4 | `node_index` | `u32` | little | spec | node_index u32 |
| 22 | 8 | `state` | `i32[2]` | little | spec | state i32[2] |
| 30 | 4 | `relation_marker` | `u32` | little | spec | relation_marker u32 |
| 34 | 4 | `relation_count` | `u32` | little | spec | relation_count u32 |
| 38 | 4 | `ordinal_key` | `u32` | little | spec | ordinal_key u32 |
| 42 | 4 | `related_marker` | `u32` | little | spec | related_marker u32 |
| 46 | 4 | `related_count` | `u32` | little | spec | related_count u32 |

## `assembly_occurrence_prefix_v28`

Spec §9 · layout: byte offsets · size: 54 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |
| 10 | 4 | `flags` | `u32` | little | spec | flags u32 |
| 14 | 4 | `flags_extension` | `u32` | little | spec | flags_extension u32 = 0 from segment major version 28 |
| 18 | 4 | `owner_reference` | `u32` | little | spec | owner_reference u32 |
| 22 | 4 | `node_index` | `u32` | little | spec | node_index u32 |
| 26 | 8 | `state` | `i32[2]` | little | spec | state i32[2] |
| 34 | 4 | `relation_marker` | `u32` | little | spec | relation_marker u32 |
| 38 | 4 | `relation_count` | `u32` | little | spec | relation_count u32 |
| 42 | 4 | `ordinal_key` | `u32` | little | spec | ordinal_key u32 |
| 46 | 4 | `related_marker` | `u32` | little | spec | related_marker u32 |
| 50 | 4 | `related_count` | `u32` | little | spec | related_count u32 |

## `assembly_occurrence_state_prefix`

Spec §9 · layout: byte offsets · size: 14 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `next_reference` | `u32` | little | spec | next_reference u32 |
| 10 | 4 | `flags` | `u32` | little | spec | flags u32 |

## `design_view_representation_head`

Spec §9 · layout: byte offsets · size: 6 B

The counted UTF-16LE name follows at offset 6; the list prefix after it has no fixed offset.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `kind` | `u16` | little | spec | kind u16 |

## `design_view_visibility_off_prefix`

Spec §9 · layout: byte offsets · size: 22 B

The occurrence keys follow at offset 22, then one zero trailer byte; later bytes are retained.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `kind` | `u16` | little | spec | kind u16 |
| 6 | 2 | `list_marker` | `u16` | little | spec | list_marker u16 = 2 · value `2` |
| 8 | 2 | `list_signature` | `u16` | little | spec | list_signature u16 = 0x3000 · value `12288` |
| 10 | 4 | `key_count` | `u32` | little | spec | key_count u32 >= 1 |
| 14 | 8 | `key_metadata` | `u32[2]` | little | spec | key_metadata u32[2] |

## `pm_dc_loft_section`

Spec §13.2 · layout: byte offsets · size: 59 B

Offsets hold in segment major versions 15 through 22 with an empty type-7 map. Later versions extend the embedded content header as section 12 states.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `content_header` | `bytes[22]` | little | spec | content_header |
| 22 | 4 | `entity_link_reference` | `u32` | little | spec | entity_link_reference u32 |
| 26 | 1 | `value` | `u8` | little | spec | value u8 |
| 27 | 4 | `condition_reference` | `u32` | little | spec | condition_reference u32 |
| 31 | 8 | `parameter_references` | `u32[2]` | little | spec | parameter_references u32[2] |
| 39 | 4 | `map_value` | `u32` | little | spec | map_value u32 = 0 |
| 43 | 4 | `map_marker` | `u32` | little | spec | map_marker u32 = 0x30000007 |
| 47 | 4 | `map_count` | `u32` | little | spec | map_count u32 = 0 |
| 51 | 4 | `source_value` | `u32` | little | spec | source_value u32 |
| 55 | 4 | `option_reference` | `u32` | little | spec | option_reference u32 |

## `pm_dc_point_paired_tail`

Spec §12 · layout: byte offsets · size: 76 B

Offsets are relative to the state word after the point's endpoint-of and center-of lists.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/sketch/point_tail.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `u32` | little | spec | state u32 · value `2` |
| 4 | 4 | `scalar_marker` | `u16[2]` | little | spec | Marker words are 2 and 0x3000. |
| 8 | 4 | `scalar_count` | `u32` | little | spec | scalar_count u32 · value `2` |
| 12 | 8 | `scalar_metadata` | `u32[2]` | little | spec | scalar_metadata u32[2] |
| 20 | 16 | `scalar_values` | `f64[2]` | little | spec | scalar_values f64[2] |
| 36 | 4 | `operand_marker` | `u16[2]` | little | spec | Marker words are 2 and 0x3000. |
| 40 | 4 | `operand_count` | `u32` | little | spec | operand_count u32 · value `2` |
| 44 | 8 | `operand_metadata` | `u32[2]` | little | spec | operand_metadata u32[2] |
| 52 | 4 | `first_auxiliary_marker` | `u16[2]` | little | spec | Marker words are 2 and 0x3000. |
| 56 | 4 | `first_auxiliary_count` | `u32` | little | spec | first_auxiliary_count u32 · value `0` |
| 60 | 4 | `first_operand` | `u32` | little | spec | first_operand u32 |
| 64 | 4 | `second_auxiliary_marker` | `u16[2]` | little | spec | Marker words are 2 and 0x3000. |
| 68 | 4 | `second_auxiliary_count` | `u32` | little | spec | second_auxiliary_count u32 · value `0` |
| 72 | 4 | `second_operand` | `u32` | little | spec | second_operand u32 |

## `pm_dc_edge_selector_prefix`

Spec §13 · layout: byte offsets · size: 22 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `i32` | little | spec | The fixed selector prefix after the content header contains |
| 4 | 1 | `enabled` | `u8` | little | spec | The fixed selector prefix after the content header contains |
| 5 | 1 | `form` | `u8` | little | spec | The fixed selector prefix after the content header contains |
| 6 | 4 | `body_reference` | `u32` | little | spec | The fixed selector prefix after the content header contains |
| 10 | 4 | `topology` | `u32` | little | spec | The fixed selector prefix after the content header contains |
| 14 | 4 | `qualifier` | `u32` | little | spec | The fixed selector prefix after the content header contains |
| 18 | 4 | `naming_form` | `u32` | little | spec | The fixed selector prefix after the content header contains |

## `meta_segment_pair`

Spec §13.4 · layout: byte offsets · size: 32 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/base.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `segment_id` | `bytes[16]` | little | spec | segment_id:bytes[16] |
| 16 | 16 | `revision_id` | `bytes[16]` | little | spec | revision_id:bytes[16] |

## `meta_external_revision`

Spec §13.4 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/base.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `revision_id` | `bytes[16]` | little | spec | revision_id:bytes[16] |
| 16 | 2 | `segment_pair_index` | `u16` | little | spec | segment_pair_index:u16 |
| 18 | 2 | `reference_count` | `u16` | little | spec | reference_count:u16 |

## `meta_external_reference`

Spec §13.4 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/base.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `revision_index` | `u32` | little | spec | revision_index:u32 |
| 4 | 2 | `key_extension` | `u16` | little | spec | key_extension:u16 |
| 6 | 2 | `header_id` | `u16` | little | spec | header_id:u16 |

## `kernel_entity_link`

Spec §13 · layout: byte offsets · size: 23 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id u16 |
| 6 | 4 | `selection_source` | `u32` | little | spec | selection_source u32 |
| 10 | 1 | `state` | `u8` | little | spec | state u8 |
| 11 | 4 | `body_reference` | `u32` | little | spec | body_reference u32 |
| 15 | 4 | `entity_id` | `u32` | little | spec | entity_id u32 |
| 19 | 4 | `entity_type` | `u32` | little | spec | entity_type u32 |

## `pm_dc_spline_curve_prefix`

Spec §12 · layout: byte offsets · size: 28 B

Offsets are relative to each spline curve, after the spline record's variable lists.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/sketch/spline.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `form` | `u32` | little | spec | form u32 · value `1` |
| 4 | 4 | `degree` | `u32` | little | spec | degree u32 |
| 8 | 8 | `tolerance` | `f64` | little | spec | tolerance f64 |
| 16 | 4 | `knot_count` | `u32` | little | spec | knot_count u32 |
| 20 | 4 | `knot_value` | `u32` | little | spec | knot_value u32 |
| 24 | 4 | `scalar_width` | `u32` | little | spec | scalar_width u32 · value `8` |

## `pm_dc_spline_curve_tail`

Spec §12 · layout: byte offsets · size: 44 B

Offsets are relative to the byte after the counted pole array.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/sketch/spline.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `pole_tolerance` | `f64` | little | spec | pole_tolerance f64 |
| 8 | 8 | `values` | `u32[2]` | little | spec | values u32[2] |
| 16 | 16 | `interval` | `f64[2]` | little | spec | interval f64[2] |
| 32 | 8 | `terminal_value` | `f64` | little | spec | terminal_value f64 |
| 40 | 4 | `scalar_width` | `u32` | little | spec | scalar_width u32 · value `8` |

## `pm_dc_plate_side_fields`

Spec §13 · layout: byte offsets · size: 4 B

Offsets start after the versioned content header and stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `type_value` | `i16` | little | spec | Enumeration type; 2 for the plate thickness side. |
| 2 | 2 | `value` | `u16` | little | spec | 0 selects thickness along the face-sketch normal. |

## `pm_dc_sheet_metal_attachment_fields`

Spec §13.5 · layout: byte offsets · size: 47 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `control` | `u32` | little | spec | Attachment control fields are: |
| 4 | 4 | `thickness` | `u32` | little | spec | Attachment control fields are: |
| 8 | 4 | `first_mode` | `u32` | little | spec | Attachment control fields are: |
| 12 | 8 | `directions` | `u32[2]` | little | spec | Attachment control fields are: |
| 20 | 4 | `radius` | `u32` | little | spec | Attachment control fields are: |
| 24 | 4 | `second_mode` | `u32` | little | spec | Attachment control fields are: |
| 28 | 4 | `relief_width` | `u32` | little | spec | Attachment control fields are: |
| 32 | 4 | `minimum_remnant` | `u32` | little | spec | Attachment control fields are: |
| 36 | 4 | `first_option` | `u32` | little | spec | Attachment control fields are: |
| 40 | 3 | `states` | `u8[3]` | little | spec | Attachment control fields are: |
| 43 | 4 | `second_option` | `u32` | little | spec | Attachment control fields are: |

## `pm_dc_sheet_metal_attachment_extension`

Spec §13.5 · layout: byte offsets · size: 4 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `extension` | `u32` | little | spec | adds one terminal `extension:u32` |

## `pm_dc_sheet_metal_relief_fields`

Spec §13.5 · layout: byte offsets · size: 24 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `control` | `u32` | little | spec | control u32 |
| 4 | 4 | `width` | `u32` | little | spec | width u32 |
| 8 | 4 | `depth` | `u32` | little | spec | depth u32 |
| 12 | 4 | `form` | `u32` | little | spec | form u32 |
| 16 | 4 | `minimum_remnant` | `u32` | little | spec | minimum_remnant u32 |
| 20 | 4 | `seam_gap` | `u32` | little | spec | seam_gap u32 |

## `pm_dc_sheet_metal_transition_fields`

Spec §13.5 · layout: byte offsets · size: 12 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `form` | `u32` | little | spec | form u32 |
| 4 | 4 | `control` | `u32` | little | spec | control u32 |
| 8 | 4 | `radius` | `u32` | little | spec | radius u32 |

## `pm_dc_sheet_metal_corner_fields`

Spec §13.5 · layout: byte offsets · size: 36 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `control` | `u32` | little | spec | Corner control fields are: |
| 4 | 4 | `gap` | `u32` | little | spec | Corner control fields are: |
| 8 | 4 | `size` | `u32` | little | spec | Corner control fields are: |
| 12 | 4 | `relief_form` | `u32` | little | spec | Corner control fields are: |
| 16 | 4 | `corner_form` | `u32` | little | spec | Corner control fields are: |
| 20 | 4 | `secondary_form` | `u32` | little | spec | Corner control fields are: |
| 24 | 4 | `jacobi_radius` | `u32` | little | spec | Corner control fields are: |
| 28 | 4 | `value` | `u32` | little | spec | Corner control fields are: |
| 32 | 4 | `option` | `u32` | little | spec | Corner control fields are: |

## `pm_dc_sheet_metal_selection_wrapper`

Spec §13.5 · layout: byte offsets · size: 4 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `edges` | `u32` | little | spec | followed by `edges:u32` |

## `pm_dc_sheet_metal_plate_control`

Spec §13.5 · layout: byte offsets · size: 4 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `value` | `u32` | little | spec | plate scalar control contains `value:u32` |

## `pm_dc_sheet_metal_bend_control`

Spec §13.5 · layout: byte offsets · size: 5 B

Offsets start after the versioned content header and stamps, or at the terminal extension.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `value` | `u32` | little | spec | extended bend scalar control contains `value:u32` and `state:u8` |
| 4 | 1 | `state` | `u8` | little | spec | extended bend scalar control contains `value:u32` and `state:u8` |

## `pm_dc_sheet_metal_attachment_state_fields`

Spec §13.5 · layout: byte offsets · size: 32 B

Offsets start after the state content header without stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `i32` | little | spec | state i32 |
| 4 | 4 | `thickness` | `u32` | little | spec | thickness u32 |
| 8 | 4 | `radius` | `u32` | little | spec | radius u32 |
| 12 | 4 | `attachment` | `u32` | little | spec | attachment u32 |
| 16 | 4 | `relief_width` | `u32` | little | spec | relief_width u32 |
| 20 | 4 | `minimum_remnant` | `u32` | little | spec | minimum_remnant u32 |
| 24 | 8 | `options` | `u32[2]` | little | spec | options u32[2] |

## `pm_dc_sheet_metal_enum_fields`

Spec §13.5 · layout: byte offsets · size: 4 B

Offsets start after the versioned content header and stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `type_value` | `i16` | little | spec | followed by `type_value:i16` and `value:u16` |
| 2 | 2 | `value` | `u16` | little | spec | followed by `type_value:i16` and `value:u16` |

## `pm_dc_sheet_metal_bend_control_base`

Spec §13.5 · layout: byte offsets · size: 4 B

Offsets start after the versioned content header and stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `value` | `u32` | little | spec | base bend scalar control contains `value:u32` |

## `pm_dc_sheet_metal_style_operand_prefix`

Spec §13.5 · layout: byte offsets · size: 90 B

Prefix of the segment-major-29 and segment-major-31 sheet-metal style record; the record continues after this prefix.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal/style.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 30 | `prefix` | `bytes[30]` | little | spec | prefix bytes[30] |
| 30 | 4 | `thickness` | `u32` | little | spec | thickness u32 |
| 34 | 4 | `bend_radius` | `u32` | little | spec | bend_radius u32 |
| 38 | 4 | `bend_relief_form` | `u32` | little | spec | bend_relief_form u32 |
| 42 | 4 | `bend_relief_width` | `u32` | little | spec | bend_relief_width u32 |
| 46 | 4 | `bend_relief_depth` | `u32` | little | spec | bend_relief_depth u32 |
| 50 | 4 | `corner_relief_form` | `u32` | little | spec | corner_relief_form u32 |
| 54 | 4 | `corner_relief_size` | `u32` | little | spec | corner_relief_size u32 |
| 58 | 4 | `seam_gap` | `u32` | little | spec | seam_gap u32 |
| 62 | 4 | `minimum_remnant` | `u32` | little | spec | minimum_remnant u32 |
| 66 | 4 | `option` | `u32` | little | spec | option u32 |
| 70 | 12 | `values` | `u32[3]` | little | spec | values u32[3] |
| 82 | 4 | `transition_form` | `u32` | little | spec | transition_form u32 |
| 86 | 4 | `transition_radius` | `u32` | little | spec | transition_radius u32 |

## `pm_dc_sheet_metal_style_operand_prefix_legacy`

Spec §13.5 · layout: byte offsets · size: 86 B

Prefix of the segment-major-20 and segment-major-23 sheet-metal style record; the record continues after this prefix.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal/style.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 26 | `prefix` | `bytes[26]` | little | spec | prefix bytes[26] |
| 26 | 4 | `thickness` | `u32` | little | spec | thickness u32 |
| 30 | 4 | `bend_radius` | `u32` | little | spec | bend_radius u32 |
| 34 | 4 | `bend_relief_form` | `u32` | little | spec | bend_relief_form u32 |
| 38 | 4 | `bend_relief_width` | `u32` | little | spec | bend_relief_width u32 |
| 42 | 4 | `bend_relief_depth` | `u32` | little | spec | bend_relief_depth u32 |
| 46 | 4 | `corner_relief_form` | `u32` | little | spec | corner_relief_form u32 |
| 50 | 4 | `corner_relief_size` | `u32` | little | spec | corner_relief_size u32 |
| 54 | 4 | `seam_gap` | `u32` | little | spec | seam_gap u32 |
| 58 | 4 | `minimum_remnant` | `u32` | little | spec | minimum_remnant u32 |
| 62 | 4 | `option` | `u32` | little | spec | option u32 |
| 66 | 12 | `values` | `u32[3]` | little | spec | values u32[3] |
| 78 | 4 | `transition_form` | `u32` | little | spec | transition_form u32 |
| 82 | 4 | `transition_radius` | `u32` | little | spec | transition_radius u32 |

## `pm_dc_loft_property_slots`

Spec §13.2 · layout: ordered slots (no stated byte offsets) · size: not stated

Non-null property references select the stated record types. Slot numbers are property-list indices, not byte offsets.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `slot_0` | `u32` | little | spec | Ordered section list. |
| 1 | `slot_1` | `u32` | little | spec | Part operation. |
| 2 | `slot_2` | `u32` | little | spec | Boolean construction control. |
| 3 | `slot_6` | `u32` | little | spec | Surface-loft result body wrapper. |
| 4 | `slot_7` | `u32` | little | spec | Auxiliary loft-selection list. |
| 5 | `slot_9` | `u32` | little | spec | Boolean construction control. |
| 6 | `slot_10` | `u32` | little | spec | Loft construction enumeration. |
| 7 | `slot_13` | `u32` | little | spec | Solid-loft result object collection. |

Cross-checked against code:

- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 0 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 1 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 2 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 6 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 7 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 9 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 10 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 13 selects the record types in the specification.

## `pm_dc_positional_loft_property_vector`

Spec §13.2 · layout: ordered slots (no stated byte offsets) · size: 14 B

The positional solid-loft operand pattern. Slot numbers are reference-list indices.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/controls.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `properties` | `u32[14]` | little | spec | The positional two-section solid-loft form has 14 property slots |

Cross-checked against code:

- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/controls.rs` — The positional property vector has 14 slots.

## `pm_dc_sweep_property_slots`

Spec §13.2 · layout: ordered slots (no stated byte offsets) · size: not stated

Non-null property references select the stated record types. Slot numbers are property-list indices, not byte offsets.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `slot_0` | `u32` | little | spec | Boundary patch. |
| 1 | `slot_1` | `u32` | little | spec | Path selection. |
| 2 | `slot_2` | `u32` | little | spec | Part operation. |
| 3 | `slot_3` | `u32` | little | spec | Angular parameter. |
| 4 | `slot_4` | `u32` | little | spec | Extent enumeration. |
| 5 | `slot_7` | `u32` | little | spec | Feature dimensions. |
| 6 | `slot_8` | `u32` | little | spec | Sweep construction enumeration. |
| 7 | `slot_9` | `u32` | little | spec | Sweep orientation enumeration. |
| 8 | `slot_10` | `u32` | little | spec | Sweep scaling enumeration. |
| 9 | `slot_15` | `u32` | little | spec | Result object collection. |

Cross-checked against code:

- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 0 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 1 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 2 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 3 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 4 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 7 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 8 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 9 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 10 selects the record types in the specification.
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/slots.rs` — Slot 15 selects the record types in the specification.

## `pm_dc_generated_face_attribute`

Spec §13.2 · layout: ordered slots (no stated byte offsets) · size: not stated

Ordered ASM typed attribute values.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/loft_sweep/projection/evidence.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `common_fields` | `array` | little | spec | Five common attribute fields. |
| 1 | `family` | `text` | little | spec | a family string |
| 2 | `version_schema_state` | `array` | little | spec | Three integer tokens: 1, 44, 1. |
| 3 | `generator` | `token_stream` | little | spec | `generator` is the generating feature's content-header source index |
| 4 | `sentinel` | `token_stream` | little | spec | `generator`, and −1 |
| 5 | `family_payload` | `array` | little | spec | Family-selected integer tokens. |

## `pm_dc_feature_dimension_prefix`

Spec §13 · layout: byte offsets · size: 6 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/revolve.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value:u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id:u16 |

## `pm_dc_feature_dimension_tail`

Spec §13 · layout: byte offsets · size: 9 B

The tail ends the record after the annotation list data.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/revolve.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 5 | `annotation` | `bytes[5]` | n/a | spec | five annotation bytes |
| 5 | 4 | `parameter` | `u32` | little | spec | parameter reference |

## `pm_dc_feature_dimension_list_prefix`

Spec §13 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/revolve.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `u32` | little | spec | marker:u32 = 0x30000002 · value `805306370` |
| 4 | 4 | `item_count` | `u32` | little | spec | item_count:u32 |

## `meta_reference_header`

Spec §3 · layout: byte offsets · size: 3 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/records/references.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `key` | `u16` | little | spec | key:u16 |
| 2 | 1 | `tag` | `u8` | little | spec | tag:u8 |

## `drawing_sheet_prefix`

Spec §15 · layout: byte offsets · size: 30 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 14 | `header` | `bytes[14]` | little | spec | The sheet prefix contains |
| 14 | 16 | `identifier` | `bytes[16]` | little | spec | The sheet prefix contains |

## `drawing_view_prefix`

Spec §15 · layout: byte offsets · size: 30 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 22 | `header` | `bytes[22]` | little | spec | The view prefix contains |
| 22 | 4 | `value_0` | `u32` | little | spec | The view prefix contains |
| 26 | 4 | `value_1` | `u32` | little | spec | The view prefix contains |

## `drawing_sheet_controls`

Spec §15 · layout: byte offsets · size: 24 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 24 | `controls` | `bytes[24]` | little | spec | The sheet controls contain |

## `drawing_model_reference_prefix`

Spec §15 · layout: byte offsets · size: 49 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 49 | `prefix` | `bytes[49]` | little | spec | The modeled-document browser prefix contains |

## `drawing_cached_view_prefix`

Spec §15 · layout: byte offsets · size: 32 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 28 | `prefix` | `bytes[28]` | little | spec | The cached-view prefix contains |
| 28 | 4 | `view_reference` | `u32` | little | spec | The cached-view prefix contains |

## `drawing_sheet_property_list`

Spec §15 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `marker` | `u16` | little | spec | The sheet property-list prefix contains · value `2` |
| 2 | 2 | `signature` | `u16` | little | spec | The sheet property-list prefix contains · value `12288` |
| 4 | 4 | `count` | `u32` | little | spec | The sheet property-list prefix contains · value `6` |
| 8 | 8 | `metadata` | `u32[2]` | little | spec | The sheet property-list prefix contains |

## `drawing_view_operand_list`

Spec §15 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `marker` | `u16` | little | spec | The view operand-list prefix contains · value `2` |
| 2 | 2 | `signature` | `u16` | little | spec | The view operand-list prefix contains · value `12288` |
| 4 | 4 | `count` | `u32` | little | spec | The view operand-list prefix contains · value `19` |
| 8 | 8 | `metadata` | `u32[2]` | little | spec | The view operand-list prefix contains |

## `meta_reference_identifier`

Spec §3 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/records/references.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `identifier` | `bytes[16]` | little | spec | Tags 1 and 2 carry a 16-byte identifier |

## `meta_reference_pair`

Spec §3 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/records/references.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `values` | `u32[2]` | little | spec | Tag 3 carries two u32 values |

## `drawing_sheet_tail_prefix`

Spec §15 · layout: byte offsets · size: 14 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 14 | `prefix` | `bytes[14]` | little | spec | The sheet tail prefix contains |

## `drawing_sheet_terminal`

Spec §15 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `terminal` | `bytes[8]` | little | spec | The sheet terminal contains |

## `drawing_view_operands`

Spec §15 · layout: ordered slots (no stated byte offsets) · size: not stated

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `operand_0` | `u32` | little | spec | \| 0 \| `2e187506d2111f5e6000b3856c2387b0` \| |
| 1 | `operand_1` | `u32` | little | spec | \| 1 \| `9ff49f1dd11101e96000a8ade3bc87b0` \| |
| 2 | `operand_2` | `u32` | little | spec | \| 2 \| `3edf52ced011d0d20008ccbc0663dc09` \| |
| 3 | `operand_3` | `u32` | little | spec | \| 3 \| `3edf52ced011d0d20008ccbc0663dc09` \| |
| 4 | `operand_4` | `u32` | little | spec | \| 4 \| `4debf5fad41120726000bebead9287b0` \| |
| 5 | `operand_5` | `u32` | little | spec | \| 5 \| `35df52ced011d0d20008ccbc0663dc09` \| |
| 6 | `operand_6` | `u32` | little | spec | \| 6 \| `474dd06b2047478c9da481bcfae149d5` \| |
| 7 | `operand_7` | `u32` | little | spec | \| 7 \| `229630f7d111a466600027a5602d6bb0` \| |
| 8 | `operand_8` | `u32` | little | spec | \| 8 \| `5c42cb22d111c557600037bd861c3cb0` \| |
| 9 | `operand_9` | `u32` | little | spec | \| 9 \| `284d8790d011f8d10008cabc0663dc09` \| |
| 10 | `operand_10` | `u32` | little | spec | \| 10 \| `284d8790d011f8d10008cabc0663dc09` \| |
| 11 | `operand_11` | `u32` | little | spec | \| 11 \| `ebf44026d111e457600037bd861c3cb0` \| |
| 12 | `operand_12` | `u32` | little | spec | \| 12 \| `264d8790d011f8d10008cabc0663dc09` \| |
| 13 | `operand_13` | `u32` | little | spec | \| 13 \| `264d8790d011f8d10008cabc0663dc09` \| |
| 14 | `operand_14` | `u32` | little | spec | \| 14 \| `e9ee786bd11195a460009dade3bc87b0` \| |
| 15 | `operand_15` | `u32` | little | spec | \| 15 \| `e9ee786bd11195a460009dade3bc87b0` \| |
| 16 | `operand_16` | `u32` | little | spec | \| 16 \| `2e187506d2111f5e6000b3856c2387b0` \| |
| 17 | `operand_17` | `u32` | little | spec | \| 17 \| `9ff49f1dd11101e96000a8ade3bc87b0` \| |
| 18 | `operand_18` | `u32` | little | spec | \| 18 \| `35df52ced011d0d20008ccbc0663dc09` \| |

Cross-checked against code:

- `crates/cadmpeg-codec-inventor/src/drawing/records.rs` — The view has nineteen typed operand positions.

## `drawing_view_header`

Spec §15 · layout: byte offsets · size: 22 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `header_value` | `u32` | little | spec | header_value:u32 |
| 4 | 2 | `header_id` | `u16` | little | spec | header_id:u16 |
| 6 | 16 | `remainder` | `bytes[16]` | little | spec | remainder:bytes[16] |

## `drawing_main_view_fixed`

Spec §15 · layout: byte offsets · size: 136 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 30 | `prefix` | `bytes[30]` | little | spec | The view prefix contains |
| 30 | 16 | `operand_list` | `bytes[16]` | little | spec | The view operand-list prefix contains |
| 46 | 76 | `operands` | `u32[19]` | little | spec | Its nineteen ordered packed references follow |
| 122 | 2 | `word` | `u16` | little | spec | word:u16 |
| 124 | 8 | `values` | `u32[2]` | little | spec | values:u32[2] |
| 132 | 4 | `text_length` | `u32` | little | spec | The counted text length is a u32 immediately before its UTF-16LE code units. |

## `drawing_alternate_view_fixed`

Spec §15 · layout: byte offsets · size: 140 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/drawing/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 30 | `prefix` | `bytes[30]` | little | spec | The view prefix contains |
| 30 | 16 | `operand_list` | `bytes[16]` | little | spec | The view operand-list prefix contains |
| 46 | 76 | `operands` | `u32[19]` | little | spec | Its nineteen ordered packed references follow |
| 122 | 2 | `word` | `u16` | little | spec | word:u16 |
| 124 | 8 | `values` | `u32[2]` | little | spec | values:u32[2] |
| 132 | 4 | `extension` | `u32` | little | spec | a u32 extension in the alternate-view form |
| 136 | 4 | `text_length` | `u32` | little | spec | The counted text length is a u32 immediately before its UTF-16LE code units. |

## `pm_dc_feature_dimension_list_metadata`

Spec §13 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/revolve.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `values` | `u32[2]` | little | spec | two u32 metadata words |

## `pm_dc_sheet_metal_corner_property_slots`

Spec §13.5 · layout: ordered slots (no stated byte offsets) · size: not stated

Slot numbers index the property reference list. Slot 6 is absent in the six-property form.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/sheet_metal/corner.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `slot_0` | `u32` | little | spec | \| 0 \| Corner control shared with plate slot 9 \| |
| 1 | `slot_1` | `u32` | little | spec | \| 1 \| Surface-body property shared with plate slot 6 in the six-property form; null in the seven-property form \| |
| 2 | `slot_2` | `u32` | little | spec | \| 2 \| Feature dimensions \| |
| 3 | `slot_3` | `u32` | little | spec | \| 3 \| Attachment selection list shared with plate slot 12 \| |
| 4 | `slot_4` | `u32` | little | spec | \| 4 \| Plate profile selection shared with plate slot 0 \| |
| 5 | `slot_5` | `u32` | little | spec | \| 5 \| Null \| |
| 6 | `slot_6` | `u32` | little | spec | \| 6 \| Result object collection; absent in the six-property form \| |

## `pm_dc_mirror_selection_tail`

Spec §13 · layout: byte offsets · size: 8 B

The zero-option mirror form. Offsets are relative to the end of the versioned content header and stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/mirror.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `control` | `u32` | little | spec | Mirror auxiliary payloads follow the versioned content header and stamps. · value `805306372` |
| 4 | 4 | `value` | `u32` | little | spec | Mirror auxiliary payloads follow the versioned content header and stamps. · value `0` |

## `pm_dc_mirror_options_tail`

Spec §13 · layout: byte offsets · size: 9 B

The zero-option mirror form. Offsets are relative to the end of the versioned content header and stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/mirror.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 9 | `options` | `bytes[9]` | little | spec | Mirror auxiliary payloads follow the versioned content header and stamps. · value `[0, 0, 0, 0, 0, 0, 0, 0, 0]` |

## `pm_dc_body_motion_properties`

Spec §13 · layout: ordered slots (no stated byte offsets) · size: 12 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/motion.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `selected_bodies` | `u32` | little | spec | Slot 0 is an object collection |
| 1 | `transforms` | `u32` | little | spec | Slot 1 is a transform list |
| 2 | `dimensions` | `u32` | little | spec | Slot 2 contains feature dimensions |

## `pm_dc_face_motion_properties`

Spec §13 · layout: ordered slots (no stated byte offsets) · size: 52 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/motion.rs`

| # | Slot | Type | Endian | Src | Meaning |
| -: | ---- | ---- | ------ | --- | ------- |
| 0 | `selected_faces` | `u32` | little | spec | Slot 0 is a face collection |
| 1 | `mode` | `u32` | little | spec | Slot 1 is the face-motion mode |
| 2 | `mode_operand_2` | `u32` | little | spec | null slots 2 through 7 and 10 |
| 3 | `mode_operand_3` | `u32` | little | spec | null slots 2 through 7 and 10 |
| 4 | `mode_operand_4` | `u32` | little | spec | null slots 2 through 7 and 10 |
| 5 | `mode_operand_5` | `u32` | little | spec | null slots 2 through 7 and 10 |
| 6 | `mode_operand_6` | `u32` | little | spec | null slots 2 through 7 and 10 |
| 7 | `mode_operand_7` | `u32` | little | spec | null slots 2 through 7 and 10 |
| 8 | `affected_bodies` | `u32` | little | spec | Slot 8 is the selection of affected bodies |
| 9 | `dimensions` | `u32` | little | spec | Slot 9 contains feature dimensions |
| 10 | `mode_operand` | `u32` | little | spec | null slots 2 through 7 and 10 |
| 11 | `flag` | `u32` | little | spec | Slot 11 is a Boolean |
| 12 | `transform` | `u32` | little | spec | Slot 12 is the model-space motion transform |

## `pm_dc_pattern_selection_control_body`

Spec §13.6 · layout: byte offsets · size: 9 B

Offsets are relative to the end of the versioned content header.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `first_reference` | `u32` | little | spec | After the content header it stores two record references and one u8 control |
| 4 | 4 | `second_reference` | `u32` | little | spec | After the content header it stores two record references and one u8 control |
| 8 | 1 | `control` | `u8` | little | spec | After the content header it stores two record references and one u8 control |

## `pm_dc_pattern_cached_edge_body`

Spec §13.6 · layout: byte offsets · size: 103 B

Selection form 1 with curve form 23. Offsets are relative to the end of the content header without stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `i32` | little | spec | store cached line selections |
| 4 | 4 | `reverse_reference` | `u32` | little | spec | store cached line selections |
| 8 | 4 | `geometry_reference` | `u32` | little | spec | store cached line selections |
| 12 | 4 | `selection_form` | `u32` | little | spec | store cached line selections · value `1` |
| 16 | 2 | `zero` | `u16` | little | spec | store cached line selections · value `0` |
| 18 | 4 | `curve_form` | `u32` | little | spec | store cached line selections · value `23` |
| 22 | 24 | `origin` | `f64[3]` | little | spec | store cached line selections |
| 46 | 24 | `span` | `f64[3]` | little | spec | store cached line selections |
| 70 | 24 | `selection_point` | `f64[3]` | little | spec | store cached line selections |
| 94 | 1 | `line_state` | `u8` | little | spec | store cached line selections · value `1` |
| 95 | 8 | `source_references` | `u32[2]` | little | spec | store cached line selections |

## `pm_dc_pattern_cached_line_body`

Spec §13.6 · layout: byte offsets · size: 99 B

Selection form 1 with curve form 23. Offsets are relative to the end of the content header without stamps.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `i32` | little | spec | store cached line selections |
| 4 | 4 | `reverse_reference` | `u32` | little | spec | store cached line selections |
| 8 | 4 | `geometry_reference` | `u32` | little | spec | store cached line selections |
| 12 | 4 | `selection_form` | `u32` | little | spec | store cached line selections · value `1` |
| 16 | 2 | `zero` | `u16` | little | spec | store cached line selections · value `0` |
| 18 | 4 | `curve_form` | `u32` | little | spec | store cached line selections · value `23` |
| 22 | 24 | `origin` | `f64[3]` | little | spec | store cached line selections |
| 46 | 24 | `span` | `f64[3]` | little | spec | store cached line selections |
| 70 | 24 | `selection_point` | `f64[3]` | little | spec | store cached line selections |
| 94 | 1 | `line_state` | `u8` | little | spec | store cached line selections · value `1` |
| 95 | 4 | `source_references` | `u32[1]` | little | spec | store cached line selections |

## `pm_dc_pattern_suppression_prefix`

Spec §13.6 · layout: byte offsets · size: 8 B

Offsets are relative to the end of the versioned content header.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `marker` | `u16` | little | spec | stores the u16 pair `4, 0x3000` and a u32 count · value `4` |
| 2 | 2 | `marker_kind` | `u16` | little | spec | stores the u16 pair `4, 0x3000` and a u32 count · value `12288` |
| 4 | 4 | `count` | `u32` | little | spec | stores the u16 pair `4, 0x3000` and a u32 count |

## `pm_dc_pattern_suppression_pair`

Spec §13.6 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature/pattern.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `first` | `u32` | little | spec | pairs of u32 coordinates |
| 4 | 4 | `second` | `u32` | little | spec | pairs of u32 coordinates |

## `pm_dc_sheet_metal_face_plate_body_v29`

Spec §13 · layout: byte offsets · size: 28 B

Offsets are relative to the end of the state content header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `i32` | little | spec | The segment-major-29 one-plate face suffix occupies · value `-1` |
| 4 | 2 | `marker` | `u16` | little | spec | The segment-major-29 one-plate face suffix occupies · value `2` |
| 6 | 2 | `signature` | `u16` | little | spec | The segment-major-29 one-plate face suffix occupies · value `12288` |
| 8 | 4 | `count` | `u32` | little | spec | The segment-major-29 one-plate face suffix occupies · value `1` |
| 12 | 4 | `metadata0` | `u32` | little | spec | The segment-major-29 one-plate face suffix occupies · value `1` |
| 16 | 4 | `metadata1` | `u32` | little | spec | The segment-major-29 one-plate face suffix occupies · value `16` |
| 20 | 4 | `plate_reference` | `u32` | little | spec | The segment-major-29 one-plate face suffix occupies |
| 24 | 4 | `value` | `u32` | little | spec | The segment-major-29 one-plate face suffix occupies · value `0` |

## `pm_dc_sheet_metal_face_plate_bend_body_v19`

Spec §13 · layout: byte offsets · size: 32 B

Offsets are relative to the end of the state content header.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `i32` | little | spec | The segment-major-19 plate-and-bend face suffix occupies · value `-1` |
| 4 | 2 | `marker` | `u16` | little | spec | The segment-major-19 plate-and-bend face suffix occupies · value `2` |
| 6 | 2 | `signature` | `u16` | little | spec | The segment-major-19 plate-and-bend face suffix occupies · value `12288` |
| 8 | 4 | `count` | `u32` | little | spec | The segment-major-19 plate-and-bend face suffix occupies · value `2` |
| 12 | 4 | `metadata0` | `u32` | little | spec | The segment-major-19 plate-and-bend face suffix occupies · value `2` |
| 16 | 4 | `metadata1` | `u32` | little | spec | The segment-major-19 plate-and-bend face suffix occupies · value `0` |
| 20 | 4 | `plate_reference` | `u32` | little | spec | The segment-major-19 plate-and-bend face suffix occupies |
| 24 | 4 | `bend_reference` | `u32` | little | spec | The segment-major-19 plate-and-bend face suffix occupies |
| 28 | 4 | `value` | `u32` | little | spec | The segment-major-19 plate-and-bend face suffix occupies · value `0` |

## `pm_dc_work_point_fields`

Spec §13 · layout: byte offsets · size: 36 B

Offsets start after the segment-major-19 content header.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/feature.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `entity_flags` | `u32` | little | spec | `entity_flags:u32` and two u32 values |
| 4 | 8 | `values` | `u32[2]` | little | spec | segment major versions above 21 add one u32 value |
| 12 | 24 | `origin` | `f64[3]` | little | spec | Three f64 values follow: the point in internal centimetres |

## `pm_dc_open_cubic_curve_prefix`

Spec §12 · layout: byte offsets · size: 28 B

Offsets are relative to each curve array of the endpoint-bound open cubic form. The scalar lanes and fixed tail share the spline curve layout.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/sketch/spline.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `form` | `u32` | little | spec | \| Array form and degree \| `0`, `3` \| · value `0` |
| 4 | 4 | `degree` | `u32` | little | spec | \| Array form and degree \| `0`, `3` \| · value `3` |
| 8 | 8 | `tolerance` | `f64` | little | spec | \| Array tolerance \| `1e-9` \| |
| 16 | 4 | `knot_count` | `u32` | little | spec | The open cubic knot vector has `knot_count = pole_count + 4`. |
| 20 | 4 | `knot_value` | `u32` | little | spec | \| Knot value \| `knot_count` \| |
| 24 | 4 | `scalar_width` | `u32` | little | spec | scalar_width u32 = 8 · value `8` |

## `pm_dc_open_cubic_pole_prefix`

Spec §12 · layout: byte offsets · size: 24 B

Offsets are relative to the byte after the counted knot lane of the endpoint-bound open cubic form.

Parsed by:
- `crates/cadmpeg-codec-inventor/src/sketch/spline.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `knot_values` | `u32[2]` | little | spec | \| Knot values \| `0, 0` \| |
| 8 | 4 | `knot_scalar_width` | `u32` | little | spec | `scalar_width:u32 = 8` · value `8` |
| 12 | 4 | `pole_count` | `u32` | little | spec | `pole_count:u32` |
| 16 | 4 | `pole_value` | `u32` | little | spec | \| Pole value \| `8` \| · value `8` |
| 20 | 4 | `pole_scalar_width` | `u32` | little | spec | `scalar_width:u32 = 8` · value `8` |
