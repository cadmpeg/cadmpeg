<!-- Generated from docs/layouts/f3d.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `f3d` record layouts

Source of truth: [`docs/formats/f3d.md`](../../docs/formats/f3d.md).
Table source: `docs/layouts/f3d.toml`.

Covers the fixed Design-segment headers, the named solid-primitive prologue,
the ten-reference `CoilPrimitive` prologue and matrix block, and the
sheet-metal `EdgeFlange` fixed operation section (§3.1). ASM stream records are tabulated in
`docs/layouts/asm.toml`. Container and manifest layers are text grammars and
are listed under "Not tabulated".

## `indexed_design_record_header`

Spec §3.1 · layout: byte offsets · size: 11 B

The 11-byte size is the spec's own "eleven-byte indexed header". §3.1 states the segment's integers are little-endian ("a nonempty contiguous sequence of little-endian i32 values").

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `class_tag_length` | `u32` | little | spec | An indexed Design record header is `u32 class_tag_length` |
| 4 | 3 | `class_tag` | `bytes[3]` | little | spec | a three-digit ASCII dynamic-class tag |
| 7 | 4 | `record_index` | `u32` | little | spec | then `u32 record_index` |

Cross-checked against code:

- `docs/formats/f3d.md` — The 11-byte total is stated independently in the companion-record paragraph of the same section.

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

## `coil_long_scope_fixed_prologue`

Spec §3.1 · layout: byte offsets · size: 52 B

Offsets are relative to the primary indexed scope header. The two marked references repeat ordered-reference ordinals four and eight; their target records are dynamic indexed records.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 11 | `indexed_header` | `bytes[11]` | little | spec | Its eleven-byte indexed header |
| 11 | 11 | `zero_run_11` | `bytes[11]` | little | spec | Both forms store eleven zero bytes at offsets 11 through 21 |
| 22 | 4 | `operation` | `u32` | little | spec | a u32 at offset 22 |
| 26 | 4 | `structural_constant` | `u32` | little | spec | u32 `1` at offset 26 |
| 30 | 11 | `fifth_reference` | `bytes[11]` | little | spec | Marked references at offsets 30 and 41 repeat the fifth and ninth ordered scope references. |
| 41 | 11 | `ninth_reference` | `bytes[11]` | little | spec | Marked references at offsets 30 and 41 repeat the fifth and ninth ordered scope references. |

## `coil_long_scope_matrix`

Spec §3.1 · layout: byte offsets · size: 128 B

The block begins at primary indexed scope offset 77. Its final row is `(0, 0, 0, 1)`; the containing 578-byte form carries it only in the new-body envelope.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 128 | `matrix` | `f64[16]` | little | spec | stores a finite 16-value f64 matrix at offset 77 |

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
