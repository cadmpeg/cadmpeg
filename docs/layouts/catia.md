<!-- Generated from docs/layouts/catia.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `catia` record layouts

Source of truth: [`docs/formats/catia.md`](../../docs/formats/catia.md).
Table source: `docs/layouts/catia.toml`.

Covers the container headers and stream directory (§3), the `SurfacicReps`
roster rows and analytic surface records (§3.5, §5.8), the `0x60` curve-support
row (§5.5), the consolidated record framing families (§6), the outer schema
records (§7), the zero-entity `a9 03` framing (§8), and the E5 framing (§9).

§1 states the global rule: all multi-byte integers are little-endian unless
explicitly marked BE, and float coordinates are in millimetres. Records that use
the big-endian lane say so per field.

## Tag inventory

| Tag | Name | Payload | Meaning | Spec |
| --- | ---- | ------: | ------- | ---- |
| `FINJPL  ` | named stream block | variable | starts named stream blocks after the outer preamble; two trailing spaces are part of the marker | §4 |
| `7C 02` | source-schema string catalog | variable | total-length-framed source-schema string catalog in the outer preamble | §4 |
| `7C D9` | literal float data | variable | literal float-data bytes; not a framed record family | §4 |
| `10 24 04 ff ff 00 00 00` | standard edge-table delimiter | 0 B | delimits the standard edge table in the inner body | §4 |
| `05 08 01` | vertex XYZ record | 12 B | 15-byte vertex XYZ record: the three-byte marker plus three little-endian f32 coordinates | §4 |
| `a9 03` | zero-entity record family | variable | zero-entity native record family in the outer preamble | §4 |
| `E5 0D 03` | E5 record family | variable | E5 native record family in the preamble or a FINJPL segment | §4 |

## `outer_header`

Spec §3.1 · layout: byte offsets · size: 64 B

`directory_offset + directory_length == file_size`. The parser reads only the magic and the two directory words; the fill and flag regions are never read.

Parsed by:
- `crates/cadmpeg-codec-catia/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `magic` | `bytes[8]` | little | spec | 0x00..0x07 magic = "V5_CFV2\0" · value `"V5_CFV2\u0000"` |
| 8 | 4 | `directory_offset` | `u32` | big | spec | 0x08..0x0B directory_offset = u32 BE |
| 12 | 4 | `directory_length` | `u32` | big | spec | 0x0C..0x0F directory_length = u32 BE |
| 16 | 8 | `fill_ff` | `bytes[8]` | little | spec | 0x10..0x17 fill_ff = ff * 8 |
| 24 | 32 | `fill_00` | `bytes[32]` | little | spec | 0x18..0x37 fill_00 = 00 * 32 |
| 56 | 8 | `hdr_flags` | `bytes[8]` | little | spec | 0x38..0x3F hdr_flags = 8 raw bytes (not constant) |

## `inner_header`

Spec §3.2 · layout: byte offsets · size: 16 B

`inner` is the first `V5_CFV2\0` after outer byte 8. `diroff = inner + A`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `magic` | `bytes[8]` | little | spec | inner = first "V5_CFV2\0" after outer byte 8 |
| 8 | 4 | `directory_offset_delta` | `u32` | big | spec | A = u32be(inner + 8) # directory offset-delta |
| 12 | 4 | `directory_length` | `u32` | big | spec | B = u32be(inner + 12) # directory length |

## `stream_descriptor_header`

Spec §3.4 · layout: byte offsets · size: 84 B

Descriptor-relative. `k` extent structs of 20 bytes each follow at ds+0x54. The standard name form ends at the three-byte tail ds-3..ds (`00 00 00`); the legacy form starts at ds+0x10 and ends with the same UTF-16LE terminator, with zero fill through ds+0x50.

Parsed by:
- `crates/cadmpeg-codec-catia/src/container.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 12 | 4 | `logical_stream_length` | `u32` | big | spec | ds+0x0c : logical_stream_length (u32be) |
| 80 | 4 | `extent_count` | `u32` | big | spec | ds+0x50 : extent_count k (u32be) |

Unstated regions:

- `0..12` (12 B): The spec states no field between the descriptor start and the logical stream length at ds+0x0c.
- `16..80` (64 B): The standard name lies before ds and ends at the fixed tail ds-3..ds. In the legacy form, ds+0x10 starts the variable-length printable UTF-16LE run; its terminator and the remaining bytes through ds+0x50 are zero.

## `extent_struct`

Spec §3.4 · layout: byte offsets · size: 20 B

`inner + phys_off + phys_len <= filesize`, `phys_len != 0`, `log_off` cumulative from 0, `log_len == phys_len`, and `sum(log_len) == logical_stream_length`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `phys_off` | `u32` | big | spec | phys_off u32be # measured from the inner magic |
| 4 | 4 | `phys_len` | `u32` | big | spec | phys_len u32be |
| 8 | 4 | `log_len` | `u32` | big | spec | log_len u32be |
| 12 | 4 | `log_off` | `u32` | big | spec | log_off u32be |
| 16 | 4 | `trailing_word` | `u32` | big | spec | trailing_word u32be |

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/container.rs` — The parser's directory magic matches the stated `file[diroff : diroff+16]` value.

## `vertex_roster_row`

Spec §3.5 · layout: byte offsets · size: 7 B

The tags are unique and strictly increasing across the run.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `marker` | `u8` | little | spec | 7-byte records `54 <tag_u24le> 00 00 00` |
| 1 | 3 | `tag` | `u24` | little | spec | `54 <tag_u24le> 00 00 00` |
| 4 | 3 | `zero_run` | `bytes[3]` | little | spec | `54 <tag_u24le> 00 00 00`, with unique, strictly increasing tags |

## `freeform_surface_core`

Spec §3.5 · layout: byte offsets · size: 47 B

`f[0:3]` is the trimmed face's AABB centre, `f[3:6]` its AABB half-extents, `f[6:9]` its bounding-sphere centre, and `f[9]` the bounding-sphere radius. The containment invariant `|f[i]−f[6+i]| + f[3+i] ≤ f[9]` holds.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `tag` | `u24` | little | spec | `<tag_u24le> 00 00 00 <10×f32le> <sign_i8>` (47 bytes) |
| 3 | 3 | `zero_run` | `bytes[3]` | little | spec | `<tag_u24le> 00 00 00 <10×f32le> <sign_i8>` (47 bytes) |
| 6 | 40 | `bounds` | `f32[10]` | little | spec | `<tag_u24le> 00 00 00 <10×f32le> <sign_i8>` (47 bytes) |
| 46 | 1 | `sign` | `i8` | little | spec | `sign ∈ {+1=0x01, −1=0xff}` |

## `analytic_surface_plane`

Spec §5.8 · layout: byte offsets · size: 49 B

Record start is `marker_pos − 5`. Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>`. The payload holds BE f32 parameters; the spec states its slot order but no per-slot offsets.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `target_tag` | `u24` | little | spec | Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>`, record start = `marker_pos − 5`. |
| 3 | 1 | `zero` | `u8` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> |
| 4 | 1 | `prebyte` | `u8` | little | spec | \| plane \| `0x32` \| `0x02` \| 49 \| start+48 \| |
| 5 | 2 | `marker` | `bytes[2]` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 7 | 1 | `kind` | `u8` | little | spec | \| plane \| `0x32` \| `0x02` \| 49 \| start+48 \| |
| 48 | 1 | `sign` | `i8` | little | spec | The last byte stores a per-face orientation sign: `+1=0x01`, `−1=0xff`. |

Unstated regions:

- `8..48` (40 B): Parameter payload. §5.8 states the BE f32 slot order per kind but no per-slot offsets; the bounds lane begins at marker-relative +3.

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/families/standard/records.rs` — The parser maps the plane marker to its analytic surface kind.
- `crates/cadmpeg-codec-catia/src/families/standard/records.rs` — The parser's kind-to-prebyte map matches the spec table row for the plane.

## `analytic_surface_cylinder`

Spec §5.8 · layout: byte offsets · size: 73 B

Cylinder and cone share prebyte and length; the kind byte distinguishes them. Payload slots are `[px py pz ax ay radius]` as BE f32.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `target_tag` | `u24` | little | spec | Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 3 | 1 | `zero` | `u8` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> |
| 4 | 1 | `prebyte` | `u8` | little | spec | \| cylinder \| `0x33` \| `0x1a` \| 73 \| start+72 \| |
| 5 | 2 | `marker` | `bytes[2]` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 7 | 1 | `kind` | `u8` | little | spec | \| cylinder \| `0x33` \| `0x1a` \| 73 \| start+72 \| |
| 72 | 1 | `sign` | `i8` | little | spec | The last byte stores a per-face orientation sign: `+1=0x01`, `−1=0xff`. |

Unstated regions:

- `8..72` (64 B): Parameter payload `cylinder `00 1a 00 33 33 [px py pz ax ay radius]` as BE f32, plus the LE-f32 witness point. §5.8 gives the slot order and the marker-relative witness offset +27 but no per-slot record offsets, and §3.5 places the bounds lane at the same marker-relative +27.

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/families/standard/records.rs` — The parser maps the cylinder marker to its analytic surface kind.
- `crates/cadmpeg-codec-catia/src/families/standard/records.rs` — The parser's kind-to-prebyte map matches the shared cylinder/cone row.

## `analytic_surface_cone`

Spec §5.8 · layout: byte offsets · size: 73 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `target_tag` | `u24` | little | spec | Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 3 | 1 | `zero` | `u8` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> |
| 4 | 1 | `prebyte` | `u8` | little | spec | \| cone \| `0x34` \| `0x1a` \| 73 \| start+72 \| |
| 5 | 2 | `marker` | `bytes[2]` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 7 | 1 | `kind` | `u8` | little | spec | \| cone \| `0x34` \| `0x1a` \| 73 \| start+72 \| |
| 72 | 1 | `sign` | `i8` | little | spec | The last byte stores a per-face orientation sign: `+1=0x01`, `−1=0xff`. |

Unstated regions:

- `8..72` (64 B): Parameter payload `[apex_x apex_y apex_z ax ay semi_angle]` as BE f32; §5.8 states the slot order but no per-slot offsets.

## `analytic_surface_sphere`

Spec §5.8 · layout: byte offsets · size: 65 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `target_tag` | `u24` | little | spec | Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 3 | 1 | `zero` | `u8` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> |
| 4 | 1 | `prebyte` | `u8` | little | spec | \| sphere \| `0x35` \| `0x12` \| 65 \| start+64 \| |
| 5 | 2 | `marker` | `bytes[2]` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 7 | 1 | `kind` | `u8` | little | spec | \| sphere \| `0x35` \| `0x12` \| 65 \| start+64 \| |
| 64 | 1 | `sign` | `i8` | little | spec | The last byte stores a per-face orientation sign: `+1=0x01`, `−1=0xff`. |

Unstated regions:

- `8..64` (56 B): Parameter payload `[cx cy cz radius]` as BE f32; §5.8 states the slot order but no per-slot offsets.

## `analytic_surface_torus`

Spec §5.8 · layout: byte offsets · size: 77 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `target_tag` | `u24` | little | spec | Grammar: `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 3 | 1 | `zero` | `u8` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> |
| 4 | 1 | `prebyte` | `u8` | little | spec | \| torus \| `0x38` \| `0x1e` \| 77 \| start+76 \| |
| 5 | 2 | `marker` | `bytes[2]` | little | spec | `tag:u24le 00 <prebyte> 00 33 <kind> <payload> <sign:i8>` |
| 7 | 1 | `kind` | `u8` | little | spec | \| torus \| `0x38` \| `0x1e` \| 77 \| start+76 \| |
| 76 | 1 | `sign` | `i8` | little | spec | The last byte stores a per-face orientation sign: `+1=0x01`, `−1=0xff`. |

Unstated regions:

- `8..76` (68 B): Parameter payload `[cx cy cz ax ay major minor]` as BE f32, plus the LE-f32 witness point at marker-relative +31; §5.8 states the slot order but no per-slot offsets.

## `a_family_frame`

Spec §6 · layout: byte offsets · size: 7 B

Header only; the width-`W` header token occupies +7..+7+W and the payload starts at +7+W. `next = +7+W+payload_len`. The header token is a small repeating type code, not a per-record object id.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `family` | `u8` | little | spec | byte0 = 0xA4 + W (a5/a6/a7 for W=1/2/3) |
| 1 | 1 | `flag` | `u8` | little | spec | flag 0x03/0x13/0x83 |
| 2 | 1 | `class` | `u8` | little | spec | +2 class +3 payload_len:u32le +7 header_token (W bytes) |
| 3 | 4 | `payload_len` | `u32` | little | spec | +3 payload_len:u32le +7 header_token (W bytes) payload @ +7+W |

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/wire/records.rs` — The parser derives the family width from `data[pos] - 0xa4`, matching the stated `0xA4 + W` rule.

## `b_family_frame`

Spec §6 · layout: byte offsets · size: 4 B

Header only; the width-`W` header token occupies +4..+4+W and the payload starts at +4+W. `next = +4+W+payload_len`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `family` | `u8` | little | spec | byte0 = 0xB1 + W (b2/b3/b4) |
| 1 | 1 | `flag` | `u8` | little | spec | flag bytes 0x03/0x13/0x83 |
| 2 | 1 | `class` | `u8` | little | spec | +2 class +3 payload_len:u8 +4 header_token (W bytes) |
| 3 | 1 | `payload_len` | `u8` | little | spec | +3 payload_len:u8 +4 header_token (W bytes) payload @ +4+W |

## `a8_object_stream_frame`

Spec §6.6 · layout: byte offsets · size: 11 B

`frame_flag` is `03`, `13`, or `83`. References inside the payload are compact tokens selecting an id width (`18` selects u16, `38` selects u24).

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `family` | `u8` | little | spec | Frame: `a8 <frame_flag> <cls> |
| 1 | 1 | `frame_flag` | `u8` | little | spec | where `frame_flag` is `03`, `13`, or `83` |
| 2 | 1 | `class` | `u8` | little | spec | `a8 <frame_flag> <cls> <payload_len:u32le @+3> |
| 3 | 4 | `payload_len` | `u32` | little | spec | <payload_len:u32le @+3> <object_id:u32le @+7> |
| 7 | 4 | `object_id` | `u32` | little | spec | <object_id:u32le @+7> <payload @+11> |

## `surface_of_revolution_b2_03_2d`

Spec §5.15 · layout: byte offsets · size: 174 B

Three normalized relations hold to f64 bit-equality: `angular_lo/scale==0.5`, `(angular_hi−angular_lo)/scale==2π`, and `mean/scale==π+0.5`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 5 | 1 | `reference_token` | `u8` | little | spec | `+5` reference token (`08` or `0a`) |
| 6 | 2 | `profile_allocation_identity` | `u16` | little | spec | `+6` profile allocation identity (u16le) |
| 8 | 96 | `frame` | `f64[12]` | little | spec | `+8` 12×f64le (axis origin XYZ + three basis vectors) |
| 104 | 32 | `bounds` | `f64[4]` | little | spec | `+104` 4×f64le angular/profile bounds, then scale/flag tail |

Unstated regions:

- `0..5` (5 B): `b2 03 2d` family, class, and length bytes; the spec's offsets are stated from the record start.
- `136..174` (38 B): Scale and flag tail. The spec names it but states no field offsets; the parser asserts constants inside it at payload-relative +131..133, +141, +149, +157, +165, and +166.

## `a9_03_frame`

Spec §8 · layout: byte offsets · size: 4 B

Header only; the payload of `YY + 8` bytes follows at +4, so the record length is `YY + 12`. Records reference each other by one-based global record ordinal into the `a9 03` stream.

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/zero_entity/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `family` | `bytes[2]` | little | spec | Record framing `a9 03 XX YY <payload[YY+8]>` |
| 2 | 1 | `tag_hi` | `u8` | little | spec | `a9 03 XX YY <payload[YY+8]>` |
| 3 | 1 | `tag_lo_length_driver` | `u8` | little | spec | `record_length = YY + 12` |

## `zero_entity_edge_stride_5e1a`

Spec §8 · layout: byte offsets · size: 38 B

Each tagged allocation value is one tag byte plus a little-endian u32, so the five values run at stride 5.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 7 | 5 | `tagged_one_prefix` | `bytes[5]` | little | spec | the fixed tagged-one prefix `10 01 00 00 00` at `+7` |
| 12 | 25 | `allocations` | `bytes[25]` | little | spec | five nonzero tagged `u32le` allocation values `[T,X,Y,T−1,T−2]` at `+12,+17,+22,+27,+32` |
| 37 | 1 | `terminal` | `u8` | little | spec | terminal byte `21` at `+37` |

Unstated regions:

- `0..7` (7 B): `a9 03 5e 1a` framing bytes and the region before the tagged-one prefix; the spec states no field here.

## `zero_entity_vertex_owner_5d06`

Spec §8 · layout: byte offsets · size: 18 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 7 | 5 | `tagged_one_a` | `bytes[5]` | little | spec | containing tagged-one values at `+7` and `+12` |
| 12 | 5 | `tagged_one_b` | `bytes[5]` | little | spec | tagged-one values at `+7` and `+12` and terminal byte zero at `+17` |
| 17 | 1 | `terminal` | `u8` | little | spec | terminal byte zero at `+17` |

Unstated regions:

- `0..7` (7 B): `a9 03 5d 06` framing bytes and the region before the first tagged-one value.

## `zero_entity_pcurve_2171`

Spec §8 · layout: byte offsets · size: 125 B

One row of the §8 inline support-pcurve family table. Distinct f64 knots are followed by equally many tagged u32 multiplicities; `degree = first_multiplicity - 1` and `control_count = sum(multiplicities) - degree - 1`. The 125-byte total is the parser's required end for this tag; §8 states logical lengths only for `2145`, `2172`, and `219f`.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 67 | 16 | `knots` | `f64[2]` | little | spec | \| `2171` \| `+67,+75` \| `+83,+88` \| `+93` \| 1 / 2 \| none \| |
| 83 | 10 | `multiplicities` | `bytes[10]` | little | spec | \| `2171` \| `+67,+75` \| `+83,+88` \| `+93` \| 1 / 2 \| none \| |
| 93 | 32 | `poles` | `f64[4]` | little | spec | The same four f64 slots the family table lists as the pole start at +93; §8 names them `(u0,v0,u1,v1)` and states each of the four offsets. |

Unstated regions:

- `0..67` (67 B): `a9 03 21 71` framing and the record head. The spec's family table starts at the first distinct knot at +67 and states nothing before it.

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/families/zero_entity/records.rs` — The parser's family table carries the same knot, multiplicity, and pole offsets for this tag.

## `zero_entity_34c8_pole_grid`

Spec §8 · layout: byte offsets · size: 1176 B

This sub-layout starts at the carrier-relative pole-grid offset +167. The variable knot and dimension lanes before it are bounded by this fixed continuation boundary.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1176 | `poles` | `f64[147]` | little | derived | 49 poles × 3 coordinates, with a 24-byte f64le XYZ stride; offset 0 here is carrier-relative +167. |

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/families/zero_entity/records.rs` — The parser selects the fixed carrier-relative grid offset and 7×7 control-point shape.

## `zero_entity_345e_pole_grid`

Spec §8 · layout: byte offsets · size: 840 B

This sub-layout starts at the carrier-relative pole-grid offset +141. The variable knot and dimension lanes before it are bounded by this fixed continuation boundary.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 840 | `poles` | `f64[105]` | little | derived | 35 poles × 3 coordinates, with a 24-byte f64le XYZ stride; offset 0 here is carrier-relative +141. |

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/families/zero_entity/records.rs` — The parser selects the fixed carrier-relative grid offset and 5×7 control-point shape.

## `e5_record_frame`

Spec §9 · layout: byte offsets · size: 13 B

The 13-byte header precedes the counted payload; an initial payload reference-count byte is separate from the header.

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 3 | `marker` | `bytes[3]` | little | spec | Framing `E5 0D 03 <cls> <sub> · value `[229, 13, 3]` |
| 3 | 1 | `class` | `u8` | little | spec | `E5 0D 03 <cls> <sub> <payload_size_u16le> |
| 4 | 1 | `sub` | `u8` | little | spec | <cls> <sub> <payload_size_u16le> 00 00 |
| 5 | 2 | `payload_size` | `u16` | little | spec | <payload_size_u16le> 00 00 <record_id_u32le> |
| 7 | 2 | `zero_run` | `bytes[2]` | little | spec | <payload_size_u16le> 00 00 <record_id_u32le> <payload> |
| 9 | 4 | `record_id` | `u32` | little | spec | <record_id_u32le> <payload>`, stride `payload_size + 13` |

## `numeric_pair_prefix`

Spec §7.3 · layout: byte offsets · size: 8 B

Each prefix atom is one byte. Slot zero follows at offset eight; nullable slots change subsequent offsets.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `prefix_atoms` | `bytes[2]` | little | spec | the two prefix atoms at offsets zero and one |
| 2 | 1 | `type_marker` | `u8` | little | spec | `E8` at two · value `232` |
| 3 | 2 | `type_atom` | `bytes[2]` | little | spec | `E4 07` at three |
| 5 | 1 | `separator` | `u8` | little | spec | `37` at five · value `55` |
| 6 | 1 | `layout_atom` | `u8` | little | spec | `83` at six · value `131` |
| 7 | 1 | `value_atom` | `u8` | little | spec | `81` at seven · value `129` |

## `numeric_pair_binary64`

Spec §7.3 · layout: byte offsets · size: 28 B

Both slots are binary64. Nullable slots use the variable-width production instead of this layout.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `prefix` | `bytes[8]` | little | spec | The prefix has eight bytes |
| 8 | 1 | `slot0_marker` | `u8` | little | spec | `E6` at eight and seventeen · value `230` |
| 9 | 8 | `slot0_bits` | `u64` | little | spec | the two `u64le` bit fields at nine and eighteen |
| 17 | 1 | `slot1_marker` | `u8` | little | spec | `E6` at eight and seventeen · value `230` |
| 18 | 8 | `slot1_bits` | `u64` | little | spec | the two `u64le` bit fields at nine and eighteen |
| 26 | 2 | `terminators` | `bytes[2]` | little | spec | `FE FE` at twenty-six |

## `sketch_owner_list_prefix`

Spec §7.3 · layout: byte offsets · size: 1 B

The count atom starts at payload offset one. Its variable width and the declared reference count determine the remaining list extent.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `list_marker` | `u8` | little | spec | Its first byte is `3B`; the count atom follows at payload offset one. · value `59` |

## `sketch_placement_controls`

Spec §7.3 · layout: byte offsets · size: 18 B

Token-selected program. E7 denotes one and E8 denotes zero. Components encode horizontal axis, vertical axis, and origin; origin is in millimetres.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `prefix` | `bytes[8]` | little | spec | Its complete token production is `82 86 E8 E0 07 37 8A 81 |
| 8 | 3 | `horizontal` | `bytes[3]` | little | spec | The nine components start at program offset eight |
| 11 | 3 | `vertical` | `bytes[3]` | little | spec | They encode the horizontal axis, vertical axis, and origin, in that order. |
| 14 | 3 | `origin` | `bytes[3]` | little | spec | They encode the horizontal axis, vertical axis, and origin, in that order. |
| 17 | 1 | `terminator` | `u8` | little | spec | the terminator is at offset seventeen. · value `254` |

## `value_block_7c0b`

Spec §7.4 · layout: byte offsets · size: 6 B

Header only. `declared_len` measures from the `7C0B` marker through the byte before the terminator, so the complete block occupies `declared_len + 1` bytes: payload of `declared_len - 6` bytes at +6, the `FE` terminator at `+declared_len`, then the associated `7C02` catalog.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `marker` | `bytes[2]` | little | spec | value_block := 7C 0B <declared_len:u32le> |
| 2 | 4 | `declared_len` | `u32` | little | spec | 7C 0B <declared_len:u32le> <payload[declared_len-6]> FE 7C 02 |

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/value_block.rs` — The parser checks the `FE` terminator at `pos + declared_len` and requires `7C 02` immediately after it.

## `outer_alias_row`

Spec §7.5 · layout: byte offsets · size: 24 B

Offsets are row-relative; the `01 00 04 00` marker sits at row offset 4. The lead is a u32 whose low byte is `0x01`, exact `0x8e`, or exact `0x8f`. The low 24 bits of `tag` are the persistent roster tag and the high byte remains part of the stored word.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `lead` | `u32` | little | spec | surface_alias_lead := <u32le whose low byte is 01> \| 8e 00 00 00 \| 8f 00 00 00 |
| 4 | 4 | `marker` | `bytes[4]` | little | spec | alias_row := surface_alias_lead 01 00 04 00 <tag:u32le> |
| 8 | 4 | `tag` | `u32` | little | spec | 01 00 04 00 <tag:u32le> <flag:u8> |
| 12 | 1 | `flag` | `u8` | little | spec | <tag:u32le> <flag:u8> <f1:3B> |
| 13 | 3 | `f1` | `bytes[3]` | little | spec | <flag:u8> <f1:3B> <f2:u32le> <f3:u32le> |
| 16 | 4 | `f2` | `u32` | little | spec | <f1:3B> <f2:u32le> <f3:u32le> |
| 20 | 4 | `f3` | `u32` | little | spec | <f2:u32le> <f3:u32le> |

Cross-checked against code:

- `crates/cadmpeg-codec-catia/src/object_graph.rs` — The parser masks the stored tag word to its low 24 bits, matching the stated persistent-roster-tag rule.

## `fbb_face_row`

Spec §7.4 · layout: byte offsets · size: 8 B

The leading byte of a colour-bearing FBB marker can set bit 7 without changing its face-row role. §5.2 gives the row's marker form as `(30|b0) 04 04 ff` at stride 8.

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/standard/fbb.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `bytes[4]` | little | spec | stores that face's effective color as `marker[4] A B G R` |
| 4 | 1 | `alpha` | `u8` | little | spec | `marker[4] A B G R` |
| 5 | 1 | `blue` | `u8` | little | spec | `marker[4] A B G R` |
| 6 | 1 | `green` | `u8` | little | spec | `marker[4] A B G R` |
| 7 | 1 | `red` | `u8` | little | spec | `marker[4] A B G R` |

## `gsmproject_range_frame_program`

Spec §7.3 · layout: byte offsets · size: 19 B

Complete R-selected frame program; the two compact context atoms do not assign coordinates.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `initial_terminator` | `u8` | little | spec | 19 bytes, with canonical one-byte atoms at offsets one and ten. · value `254` |
| 1 | 1 | `first_context_atom` | `u8` | little | spec | 19 bytes, with canonical one-byte atoms at offsets one and ten. |
| 2 | 8 | `first_program` | `bytes[8]` | little | spec | DC 72 E8 D4 08 37 82 FE |
| 10 | 1 | `second_context_atom` | `u8` | little | spec | 19 bytes, with canonical one-byte atoms at offsets one and ten. |
| 11 | 8 | `second_program` | `bytes[8]` | little | spec | E2 E5 E8 C0 1C 37 82 FE |

## `gsmproject_range_frame_references`

Spec §7.3 · layout: byte offsets · size: 86 B

14 ordered fixed-width reference fields, followed by the exact terminal pair.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 18 | `origin_references` | `bytes[18]` | little | spec | The first three identities supply the frame origin in millimetres |
| 18 | 54 | `matrix_references` | `bytes[54]` | little | spec | the next nine supply a row-major orthonormal matrix |
| 72 | 12 | `support_references` | `bytes[12]` | little | spec | The final two identities retain support incidences. |
| 84 | 2 | `terminal_pair` | `bytes[2]` | little | spec | 86 bytes, with 14 distinct source-ordered identities. |

## `storage_member_core`

Spec §7.6 · layout: byte offsets · size: 20 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `marker` | `bytes[4]` | little | spec | The marker is at core offset zero |
| 4 | 4 | `tag` | `u32` | little | spec | the stored tag at offset four |
| 8 | 1 | `flag` | `u8` | little | spec | flag at offset eight |
| 9 | 2 | `prefix` | `bytes[2]` | little | spec | retained prefix at offset nine |
| 11 | 4 | `slot` | `u32` | little | spec | slot at offset eleven |
| 15 | 5 | `tail` | `bytes[5]` | little | spec | retained tail at offset fifteen |

## `storage_member_direct_prefix`

Spec §7.6 · layout: byte offsets · size: 4 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `lead` | `u32` | little | spec | A direct numbered member has prefix `01 00 00 00` |

## `storage_member_nullable_prefix`

Spec §7.6 · layout: byte offsets · size: 8 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `state` | `u32` | little | spec | with state at prefix offset zero |
| 4 | 4 | `discriminator` | `u32` | little | spec | discriminator at offset four |

## `storage_member_number_prefix`

Spec §7.6 · layout: byte offsets · size: 6 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `inclusive_length` | `u16` | little | spec | the inclusive `u16le` length is at offset zero |
| 2 | 4 | `digit_count` | `u32` | little | spec | the `u32le` digit count is at offset two |

## `inline_schema_words`

Spec §7.3 · layout: byte offsets · size: 12 B

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `open_count` | `u8` | little | spec | opening count at offset zero |
| 1 | 1 | `outer_marker` | `u8` | little | spec | outer selector marker `32` at offset one |
| 2 | 4 | `outer_selector` | `u32` | little | spec | outer selector `u32le` at offset two |
| 6 | 1 | `field_marker` | `u8` | little | spec | field selector marker `32` at offset six |
| 7 | 4 | `field_selector` | `u32` | little | spec | field selector `u32le` at offset seven |
| 11 | 1 | `close_count` | `u8` | little | spec | closing count at offset eleven |

## `e5_bound_parameter`

Spec §9 · layout: byte offsets · size: 12 B

Each tuple belongs to the representation at the same position in the counted reference lane. Repeated identities remain separate entries.

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/graph.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `local_parameter` | `f64` | little | spec | the local parameter occupies bytes `0..8` |
| 8 | 4 | `arc_number_bits` | `u32` | little | spec | the arc-number bits occupy bytes `8..12` |

## `e5_nurbs_pcurve_limits`

Spec §9 · layout: byte offsets · size: 37 B

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/graph.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `lower_arc` | `u16` | little | spec | \| `+0` \| `u16le` \| lower arc number \| |
| 2 | 2 | `upper_arc` | `u16` | little | spec | \| `+2` \| `u16le` \| upper arc number \| |
| 4 | 8 | `lower_local` | `f64` | little | spec | \| `+4` \| `f64le` \| lower arc-local parameter \| |
| 12 | 8 | `upper_local` | `f64` | little | spec | \| `+12` \| `f64le` \| upper arc-local parameter \| |
| 20 | 8 | `coefficient` | `f64` | little | spec | \| `+20` \| `f64le` \| original-to-active coefficient \| |
| 28 | 8 | `shift` | `f64` | little | spec | \| `+28` \| `f64le` \| original-to-active shift \| |
| 36 | 1 | `extrapolation` | `u8` | little | spec | \| `+36` \| `u8` \| extrapolation control \| |

## `e5_nurbs_surface_limits`

Spec §9 · layout: byte offsets · size: 148 B

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 2 | `lower_u_patch` | `u16` | little | spec | \| `+0` \| `u16le` \| lower U patch number \| |
| 2 | 2 | `upper_u_patch` | `u16` | little | spec | \| `+2` \| `u16le` \| upper U patch number \| |
| 4 | 2 | `lower_v_patch` | `u16` | little | spec | \| `+4` \| `u16le` \| lower V patch number \| |
| 6 | 2 | `upper_v_patch` | `u16` | little | spec | \| `+6` \| `u16le` \| upper V patch number \| |
| 8 | 8 | `lower_u_local` | `f64` | little | spec | \| `+8` \| `f64le` \| lower U patch-local parameter \| |
| 16 | 8 | `upper_u_local` | `f64` | little | spec | \| `+16` \| `f64le` \| upper U patch-local parameter \| |
| 24 | 8 | `lower_v_local` | `f64` | little | spec | \| `+24` \| `f64le` \| lower V patch-local parameter \| |
| 32 | 8 | `upper_v_local` | `f64` | little | spec | \| `+32` \| `f64le` \| upper V patch-local parameter \| |
| 40 | 8 | `u_coefficient` | `f64` | little | spec | \| `+40` \| `f64le` \| U original-to-active coefficient \| |
| 48 | 8 | `u_shift` | `f64` | little | spec | \| `+48` \| `f64le` \| U original-to-active shift \| |
| 56 | 8 | `v_coefficient` | `f64` | little | spec | \| `+56` \| `f64le` \| V original-to-active coefficient \| |
| 64 | 8 | `v_shift` | `f64` | little | spec | \| `+64` \| `f64le` \| V original-to-active shift \| |
| 72 | 6 | `extrapolation_flags` | `u16[3]` | little | spec | \| `+72` \| `3×u16le` \| extrapolation flags \| |
| 78 | 64 | `extrapolation_continuation` | `f64[8]` | little | spec | \| `+78` \| `8×f64le` \| extrapolation continuation scalars \| |
| 142 | 6 | `extrapolation_suffix` | `u8[6]` | little | spec | \| `+142` \| `6×u8` \| extrapolation suffix controls \| |

## `e5_affine_support_tail`

Spec §9 · layout: byte offsets · size: 68 B

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/support.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `map_accuracy` | `f64` | little | spec | \| `+0` \| `f64le` \| map accuracy in millimetres \| |
| 8 | 16 | `co_limits` | `f64[2]` | little | spec | \| `+8` \| `2×f64le` \| useful co-facet limits \| |
| 24 | 8 | `scale` | `f64` | little | spec | \| `+24` \| `f64le` \| co-to-reference scale \| |
| 32 | 8 | `offset` | `f64` | little | spec | \| `+32` \| `f64le` \| co-to-reference offset \| |
| 40 | 16 | `active_reference_limits` | `f64[2]` | little | spec | \| `+40` \| `2×f64le` \| active reference limits \| |
| 56 | 8 | `fit_tolerance` | `f64` | little | spec | \| `+56` \| `f64le` \| fit tolerance in millimetres \| |
| 64 | 4 | `terminal` | `u32` | little | spec | \| `+64` \| `u32le` \| zero terminal \| |

## `e5_boundary_support_tail`

Spec §9 · layout: byte offsets · size: 12 B

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/support.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `zero_tolerance` | `f64` | little | spec | \| `+0` \| `f64le` \| zero boundary tolerance \| |
| 8 | 4 | `terminal` | `u32` | little | spec | \| `+8` \| `u32le` \| zero boundary terminal \| |

## `e5_sampled_support_prefix`

Spec §9 · layout: byte offsets · size: 25 B

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/support.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 8 | `map_accuracy` | `f64` | little | spec | \| `+0` \| `f64le` \| sampled map accuracy \| |
| 8 | 16 | `co_limits` | `f64[2]` | little | spec | \| `+8` \| `2×f64le` \| sampled co-facet limits \| |
| 24 | 1 | `count` | `u8` | little | spec | \| `+24` \| `u8` \| sample count encoded as `80+n` \| |

## `e5_sampled_support_suffix`

Spec §9 · layout: byte offsets · size: 28 B

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/e5/support.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 16 | `active_reference_limits` | `f64[2]` | little | spec | \| `+0` \| `2×f64le` \| sampled active reference limits \| |
| 16 | 8 | `fit_tolerance` | `f64` | little | spec | \| `+16` \| `f64le` \| sampled fit tolerance \| |
| 24 | 4 | `terminal` | `u32` | little | spec | \| `+24` \| `u32le` \| zero sampled terminal \| |

## `b5_loop_numeric_terminal`

Spec §6.7 · layout: byte offsets · size: 62 B

Terminal-relative offsets; all six compact controls use the canonical one-byte form.

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/b5/graph.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 1 | `lead` | `u8` | little | spec | \| `+0` \| `u8` \| compact lead control \| |
| 1 | 32 | `scalars` | `f64[4]` | little | spec | \| `+1` \| `4×f64le` \| local coordinates U0, V0, U1, V1 \| |
| 33 | 5 | `trailing_controls` | `u8[5]` | little | spec | \| `+33` \| `5×u8` \| patch IDs U0, V0, U1, V1, then compact zero \| |
| 38 | 24 | `bounds` | `f32[6]` | little | spec | \| `+38` \| `6×f32le` \| Cartesian X low/high, Y low/high, Z low/high \| |

## `surface_terminal_span_tail`

Spec §6.6 · layout: byte offsets · size: 141 B

Tail-relative offsets. The generic 133-byte and 142-byte surface tails share the prefix through offset 71.

Parsed by:
- `crates/cadmpeg-codec-catia/src/families/a5a8/records.rs`

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `controls` | `bytes[4]` | little | spec | 4*(K_U-1)+1 |
| 4 | 64 | `parameters` | `bytes[64]` | little | spec | U[-1]-U[-2] |
| 68 | 3 | `flags` | `bytes[3]` | little | spec | Preserve the fixed flag, continuation and suffix bytes. |
| 71 | 64 | `continuation` | `bytes[64]` | little | spec | eight zero `f64le` values |
| 135 | 6 | `suffix` | `bytes[6]` | little | spec | 01 00 01 00 07 07 |

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| `b5 03` frame header (§6.7) | §6.7 | §6.7 states payload grammars for about twenty `b5 03` classes but never states the frame header layout; it only says to advance by the declared frame length. The parser asserts an 8-byte header with `payload_len:u8` at +3 and `object_id:u32le` at +4, which the spec does not corroborate. Recorded in the pull request. |
| Trim records and the standard spine (§5.2, §5.3) | §5.3 | Every count in a trim packet is a variable-width `count()` atom and the spine tables are count-driven, so no field sits at a fixed offset. The stride-8 FBB row and the 15-byte vertex record are the exceptions and are tabled. |
| Analytic payload slots of `b2 03 19/28/29/2a/2b` (§5.9-§5.14) | §5.9 | These records state a total payload length and an ordered f64 slot list, but no per-slot byte offsets, and the leading compact record id has variable width in the circle case. They are slot layouts rather than byte layouts. |
| `7C09` inline object records and `7C0A` atom payloads (§7.3) | §7.3 | Reference tokens are one or two bytes depending on value, and the head forms vary in arity, so every later field position depends on preceding values. |
| Freeform surface and curve cores `a5 03 34` / `a5 03 32` (§6.1, §6.2) | §6.1 | Knot counts and pole grids drive every later position. The tail starts at a count-dependent offset. The fixed 141-byte surface tail has a separate relative layout; the 59-byte `a8 03 32` jet tail starts after the variable jet grid. |
