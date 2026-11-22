<!-- Generated from docs/layouts/solidedge.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `solidedge` record layouts

Source of truth: [`docs/formats/solidedge.md`](../../docs/formats/solidedge.md).
Table source: `docs/layouts/solidedge.toml`.

## `property_set_header`

Spec §2.1 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/settings.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `zero` | `u32` | little | spec | \| 0 \| `u32` zero word \| `0` \| · value `0` |
| 4 | 2 | `tag` | `u16` | little | spec | \| 4 \| `u16` set tag \| `0x5151` \| · value `20817` |
| 6 | 2 | `form` | `u16` | little | spec | \| 6 \| `u16` set form \| \| |
| 8 | 4 | `index` | `u32` | little | spec | \| 8 \| `u32` set index \| \| |
| 12 | 4 | `count` | `u32` | little | spec | \| 12 \| `u32` record count `N` \| \| |

## `space_map_header`

Spec §3.1 · layout: byte offsets · size: 12 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `bytes[4]` | little | spec | \| 0 \| ASCII `sseg` \| · value `"sseg"` |
| 4 | 2 | `item_count` | `u16` | little | spec | \| 4 \| `u16` item count `A` \| |
| 6 | 2 | `bodyless_count` | `u16` | little | spec | \| 6 \| `u16` count of items without a body \| |
| 8 | 2 | `next_serial` | `u16` | little | spec | \| 8 \| `u16` next free serial \| |
| 10 | 2 | `free_count` | `u16` | little | spec | \| 10 \| `u16` free-list count `F` \| |

## `space_map_item`

Spec §3.1 · layout: byte offsets · size: 18 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `length` | `u16` | little | spec | \| 0 \| `u16` length `L` \| |
| 2 | 4 | `object_offset` | `u32` | little | spec | \| 2 \| `u32` object offset in the cluster stream \| |
| 6 | 4 | `object_id` | `u32` | little | spec | \| 6 \| `u32` object identifier \| |
| 10 | 2 | `form_word` | `u16` | little | spec | \| 10 \| `u16` form word; bits 6 and above are the object's form \| |
| 12 | 2 | `word_a` | `u16` | little | spec | \| 12 \| `u16` \| |
| 14 | 2 | `word_b` | `u16` | little | spec | \| 14 \| `u16` \| |
| 16 | 2 | `flags` | `u16` | little | spec | \| 16 \| `u16` flags; bit `0x2000` selects the class rule in §3.3 \| |

## `cluster_header`

Spec §3.1 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `u32` | little | spec | A cluster stream starts with the `u32` value `0x6C90F544` · value `1821439300` |
| 4 | 4 | `object_count` | `u32` | little | spec | and a `u32` object |

## `object_frame`

Spec §3.1 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `tag` | `u16` | little | spec | \| 0 \| `u16` `0x5151` \| · value `20817` |
| 2 | 2 | `form` | `u16` | little | spec | \| 2 \| `u16` form, equal to the item's form word shifted right by 6 \| |
| 4 | 4 | `object_id` | `u32` | little | spec | \| 4 \| `u32` object identifier, equal to the item's identifier \| |

## `type_table_section_header`

Spec §3.2 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `bytes[4]` | little | spec | header that starts with ASCII `ePyT`. · value `"ePyT"` |
| 10 | 2 | `count` | `u16` | little | spec | The `u16` values at header offsets 10 |
| 14 | 2 | `count_repeat` | `u16` | little | spec | and 14 are equal and give the section's record count. |

Unstated regions:

- `4..10` (6 B): §3.2 states no field between the magic and the count.
- `12..14` (2 B): §3.2 states no field between the two counts.
- `16..20` (4 B): §3.2 states no field after the second count.

## `type_declaration`

Spec §3.2 · layout: byte offsets · size: 30 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `class_guid` | `bytes[16]` | little | spec | \| 0 \| 16-byte class GUID \| |
| 16 | 4 | `index` | `u32` | little | spec | \| 16 \| `u32` declaration index |
| 20 | 2 | `marker_one` | `u16` | little | spec | \| 20 \| `u16` `1` \| · value `1` |
| 22 | 2 | `marker_absent` | `u16` | little | spec | \| 22 \| `u16` `0xFFFF` \| · value `65535` |
| 24 | 4 | `metadata` | `u32` | little | spec | \| 24 \| `u32` metadata word \| |
| 28 | 2 | `name_len` | `u16` | little | spec | \| 28 \| `u16` class-name byte count `n` \| |

## `type_compact_record`

Spec §3.2 · layout: byte offsets · size: 26 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/psm.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `guid` | `bytes[16]` | little | spec | a 16-byte GUID and ten bytes |
| 18 | 2 | `link` | `u16` | little | spec | `u16` value `V` at record offset 18 |

Unstated regions:

- `16..18` (2 B): §3.2 states no field before the value.
- `20..26` (6 B): §3.2 states no field after the value.

## `profile`

Spec §5.1 · layout: byte offsets · size: 304 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/sketches.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 26 | 24 | `origin` | `f64[3]` | little | spec | \| 26 \| three `f64`: origin in model space \| |
| 50 | 24 | `normal` | `f64[3]` | little | spec | \| 50 \| three `f64`: vector `A`, the plane normal \| |
| 74 | 24 | `x_axis` | `f64[3]` | little | spec | \| 74 \| three `f64`: vector `B`, the local x axis \| |
| 127 | 4 | `profile_2d` | `u32` | little | spec | \| 127 \| `u32` identifier of the `CLSID_JProfile2d` object \| |
| 292 | 4 | `plane_marker_a` | `u32` | little | spec | \| 292 \| `u32` `0x10`, `u32` `0x80000000`, `u32` identifier of the plane \| · value `16` |
| 296 | 4 | `plane_marker_b` | `u32` | little | spec | \| 292 \| `u32` `0x10`, `u32` `0x80000000`, `u32` identifier of the plane \| · value `2147483648` |
| 300 | 4 | `plane` | `u32` | little | spec | \| 292 \| `u32` `0x10`, `u32` `0x80000000`, `u32` identifier of the plane \| |

Unstated regions:

- `0..26` (26 B): §5.1 states no field before the origin.
- `98..127` (29 B): §5.1 states no field between the local x axis and the profile-2d identifier.
- `131..292` (161 B): §5.1 states no field between the profile-2d identifier and the plane reference.

## `profile_2d`

Spec §5.1 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/sketches.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 4 | `member_count` | `u32` | little | spec | offset 8 is the member count `n` |
| 12 | 4 | `first_member` | `u32` | little | spec | and `n` `u32` member identifiers follow |

Unstated regions:

- `0..8` (8 B): §5.1 states no field before the member count.

## `line_2d`

Spec §5.2 · layout: byte offsets · size: 42 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/sketches.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 10 | 8 | `x0` | `f64` | little | spec | object offsets 10, 18, 26 and 34 are x0, y0, x1 and y1 in the profile |
| 18 | 8 | `y0` | `f64` | little | spec | object offsets 10, 18, 26 and 34 are x0, y0, x1 and y1 in the profile |
| 26 | 8 | `x1` | `f64` | little | spec | object offsets 10, 18, 26 and 34 are x0, y0, x1 and y1 in the profile |
| 34 | 8 | `y1` | `f64` | little | spec | object offsets 10, 18, 26 and 34 are x0, y0, x1 and y1 in the profile |

Unstated regions:

- `0..10` (10 B): §5.2 states no field before the start x.

## `circle_2d`

Spec §5.2 · layout: byte offsets · size: 34 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/sketches.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 10 | 8 | `center_x` | `f64` | little | spec | object offsets 10 and 18 are the centre x and y in the profile frame |
| 18 | 8 | `center_y` | `f64` | little | spec | object offsets 10 and 18 are the centre x and y in the profile frame |
| 26 | 8 | `radius` | `f64` | little | spec | `f64` at offset 26 is the radius. |

Unstated regions:

- `0..10` (10 B): §5.2 states no field before the centre x.

## `arc_2d`

Spec §5.2 · layout: byte offsets · size: 51 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/sketches.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 10 | 8 | `center_x` | `f64` | little | spec | object offsets 10, 18 and 26 are the centre x, y and the radius. |
| 18 | 8 | `center_y` | `f64` | little | spec | object offsets 10, 18 and 26 are the centre x, y and the radius. |
| 26 | 8 | `radius` | `f64` | little | spec | object offsets 10, 18 and 26 are the centre x, y and the radius. |
| 35 | 8 | `start_angle` | `f64` | little | spec | offsets 35 and 43 are the start and end angles, measured from the local x |
| 43 | 8 | `end_angle` | `f64` | little | spec | offsets 35 and 43 are the start and end angles, measured from the local x |

Unstated regions:

- `0..10` (10 B): §5.2 states no field before the centre x.
- `34..35` (1 B): §5.2 states no field between the radius and the start angle.

## `double_value`

Spec §6.1 · layout: byte offsets · size: 16 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/extents.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 8 | `value` | `f64` | little | spec | as the `f64` at object offset 8. |

Unstated regions:

- `0..8` (8 B): §6.1 states no field before the value.

## `dimension`

Spec §7.1 · layout: byte offsets · size: 210 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/dimension.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 144 | 4 | `marker` | `u32` | little | spec | A `CLSID_JDim` object stores the `u32` value `1`, then the identifier `T` of · value `1` |
| 148 | 4 | `target` | `u32` | little | spec | the object it dimensions at offset `p`, then a `u32` type code. The `u32` at |
| 198 | 4 | `target_repeat` | `u32` | little | spec | `p` + 50 is `T` again, and the `f64` at `p` + 54 is the value. `p` is 148 in |
| 202 | 8 | `value` | `f64` | little | spec | `p` + 50 is `T` again, and the `f64` at `p` + 54 is the value. `p` is 148 in |

Unstated regions:

- `0..144` (144 B): §7.1 states no field before the marker.
- `152..198` (46 B): §7.1 states a type code at this offset and no value between the target and its repeat; the type code is not read.

## `dimension_compact`

Spec §7.1 · layout: byte offsets · size: 206 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/dimension.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 140 | 4 | `marker` | `u32` | little | spec | A `CLSID_JDim` object stores the `u32` value `1`, then the identifier `T` of · value `1` |
| 144 | 4 | `target` | `u32` | little | spec | the object it dimensions at offset `p`, then a `u32` type code. The `u32` at |
| 194 | 4 | `target_repeat` | `u32` | little | spec | `p` + 50 is `T` again, and the `f64` at `p` + 54 is the value. `p` is 148 in |
| 198 | 8 | `value` | `f64` | little | spec | `p` + 50 is `T` again, and the `f64` at `p` + 54 is the value. `p` is 148 in |

Unstated regions:

- `0..140` (140 B): §7.1 states no field before the marker.
- `148..194` (46 B): §7.1 states a type code at this offset and no value between the target and its repeat; the type code is not read.

## `hole_prop_data`

Spec §7.2 · layout: byte offsets · size: 80 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/dressup.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 4 | `hole_type` | `u32` | little | spec | \| 8 \| `u32` hole type: `0` simple, `1` countersink, `2` counterbore, `3` tapped \| |
| 12 | 8 | `diameter` | `f64` | little | spec | \| 12 \| `f64` hole diameter; the tap drill diameter of a tapped hole \| |
| 20 | 8 | `head_diameter` | `f64` | little | spec | \| 20 \| `f64` counterbore or countersink diameter \| |
| 28 | 8 | `head_depth` | `f64` | little | spec | \| 28 \| `f64` counterbore depth \| |
| 36 | 8 | `sink_angle` | `f64` | little | spec | \| 36 \| `f64` countersink angle, in degrees \| |
| 72 | 8 | `depth` | `f64` | little | spec | \| 72 \| `f64` hole depth \| |

Unstated regions:

- `0..8` (8 B): §7.2 states no field before the hole type.
- `44..72` (28 B): §7.2 states no field between the countersink angle and the hole depth.

## `hole_2d`

Spec §7.2 · layout: byte offsets · size: 24 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/dressup.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 8 | 8 | `x` | `f64` | little | spec | values at object offsets 8 and 16 are the position x and y in the profile |
| 16 | 8 | `y` | `f64` | little | spec | values at object offsets 8 and 16 are the position x and y in the profile |

Unstated regions:

- `0..8` (8 B): §7.2 states no field before the position x.

## `hole_thread`

Spec §7.2 · layout: byte offsets · size: 1073 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/dressup.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 1053 | 8 | `minor_diameter` | `f64` | little | spec | \| 1053 \| `f64` thread minor diameter, in millimetres \| |
| 1061 | 8 | `major_diameter` | `f64` | little | spec | \| 1061 \| `f64` thread major diameter, in millimetres \| |
| 1069 | 4 | `designation_length` | `u32` | little | spec | \| 1069 \| `u32` thread designation length `k`, in UTF-16 code units \| |

Unstated regions:

- `0..1053` (1053 B): §7.2 states the fields before the thread in the hole_prop_data record.

## `topology_proxy_edge`

Spec §7.6 · layout: byte offsets · size: 559 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/edge.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 121 | 24 | `start` | `f64[3]` | little | spec | `f64` values at object offsets 121, 129 and 137 are the edge start point, |
| 145 | 24 | `direction` | `f64[3]` | little | spec | the `f64` values at offsets 145, 153 and 161 are the unit edge direction, |
| 177 | 8 | `length` | `f64` | little | spec | and the `f64` at offset 177 is the edge length. |

Unstated regions:

- `0..121` (121 B): §7.6 states no field before the edge start point.
- `169..177` (8 B): §7.6 states no field between the edge direction and the edge length.
- `185..559` (374 B): §7.6 states no field after the edge length.

## `ref_axis`

Spec §6.4 · layout: byte offsets · size: 61 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/extents.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 12 | 1 | `marker` | `u8` | little | spec | object offset 12 is `3`. · value `3` |
| 13 | 24 | `point` | `f64[3]` | little | spec | The three `f64` at offset 13 are a point on the |
| 37 | 24 | `direction` | `f64[3]` | little | spec | axis in model space, and the three `f64` at offset 37 are the unit axis |

Unstated regions:

- `0..12` (12 B): §6.4 states no field before the axis marker.

## `pattern_group`

Spec §8.1 · layout: byte offsets · size: 341 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/patterns.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 106 | 4 | `fast_pattern_part` | `u32` | little | spec | \| 106 \| `u32` identifier of the `CLSID_JMPart` object with function `JMFastPatternFunction` \| |
| 337 | 4 | `seed` | `u32` | little | spec | \| 337 \| `u32` identifier of the seed feature \| |

Unstated regions:

- `0..106` (106 B): §8.1 states no field before the fast-pattern part identifier.
- `110..337` (227 B): §8.1 states no field between the fast-pattern part identifier and the seed.

## `mirror_group`

Spec §8.2 · layout: byte offsets · size: 329 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/patterns.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 92 | 4 | `seed_count` | `u32` | little | spec | \| 92 \| `u32` seed count `c` \| |
| 96 | 4 | `first_seed` | `u32` | little | spec | \| 96 \| `c` `u32` seed feature identifiers \| |

Unstated regions:

- `0..92` (92 B): §8.2 states no field before the seed count.
- `100..329` (229 B): §8.2 states the mirror plane identifier at 325 + 4·c, after the seed list, and no other field.

## `reference_plane`

Spec §8.2 · layout: byte offsets · size: 61 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/patterns.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 13 | 24 | `point` | `f64[3]` | little | spec | is mirrored. Its object stores three `f64` at offset 13, a point on the plane, |
| 37 | 24 | `normal` | `f64[3]` | little | spec | and three `f64` at offset 37, the unit plane normal. |

Unstated regions:

- `0..13` (13 B): §8.2 states no field before the plane point.

## `references_collection`

Spec §9.1 · layout: byte offsets · size: 20 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 16 | 4 | `count` | `u32` | little | spec | The `u32` at object offset 16 is the |

Unstated regions:

- `0..16` (16 B): §9.1 states no field before the occurrence count.

## `assembly_reference`

Spec §9.2 · layout: byte offsets · size: 229 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 16 | 4 | `state` | `u32` | little | spec | \| 16 \| `u32` state word; bit `0x04` set states that the occurrence is suppressed \| |
| 113 | 4 | `site` | `u32` | little | spec | \| 113 \| `u32` identifier of the `CLSID_JSite` object of the referenced document \| |
| 125 | 72 | `rotation` | `f64[9]` | little | spec | \| 125 \| nine `f64`: the rotation `R`, row-major \| |
| 197 | 24 | `translation` | `f64[3]` | little | spec | \| 197 \| three `f64`: the translation `t`, in metres \| |
| 221 | 8 | `unit` | `f64` | little | spec | \| 221 \| `f64` `1` \| · value `1.0` |

Unstated regions:

- `0..16` (16 B): §9.2 states no field before the state word.
- `20..113` (93 B): §9.2 states no field between the state word and the site identifier.
- `117..125` (8 B): §9.2 states no field between the site identifier and the rotation.

## `file_moniker`

Spec §9.3 · layout: byte offsets · size: 22 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/assembly.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `clsid` | `bytes[16]` | little | spec | \| 0 \| the file moniker CLSID `{00000303-0000-0000-C000-000000000046}` \| · value `[3, 3, 0, 0, 0, 0, 0, 0, 192, 0, 0, 0, 0, 0, 0, 70]` |
| 16 | 2 | `up_level` | `u16` | little | spec | \| 16 \| `u16` up-level count, `0` \| |
| 18 | 4 | `path_len` | `u32` | little | spec | \| 18 \| `u32` byte count `n` \| |

## `variable_table_header`

Spec §10.1 · layout: byte offsets · size: 8 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `count` | `u32` | little | spec | It starts with a `u32` record count `n` and a `u32` |
| 4 | 4 | `version` | `u32` | little | spec | version `1`. `n` records of 24 bytes follow: · value `1` |

## `variable_record`

Spec §10.1 · layout: byte offsets · size: 24 B

Parsed by:
- `crates/cadmpeg-codec-solidedge/src/sheet_metal.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `key` | `u32` | little | spec | \| 0 \| `u32` key \| |
| 4 | 4 | `index` | `u32` | little | spec | \| 4 \| `u32` index \| |
| 8 | 4 | `object` | `u32` | little | spec | \| 8 \| `u32` identifier of the cluster-0 `Double Value Object` that holds the variable \| |
| 12 | 2 | `unit` | `u16` | little | spec | \| 12 \| `u16` unit type: `1` length in metres, `16` density, `58` unitless \| |
| 14 | 2 | `reserved` | `u16` | little | spec | \| 14 \| `u16` zero \| · value `0` |
| 16 | 8 | `value` | `f64` | little | spec | \| 16 \| `f64` value, equal to the value of the referenced object \| |
