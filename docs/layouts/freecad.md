<!-- Generated from docs/layouts/freecad.toml by
     crates/cadmpeg/tests/layout_tables.rs. Do not edit by hand;
     run `UPDATE_LAYOUT_DOCS=1 cargo test -p cadmpeg --test layout_tables`. -->

# `freecad` record layouts

Source of truth: [`docs/formats/freecad_fcstd.md`](../../docs/formats/freecad_fcstd.md).
Table source: `docs/layouts/freecad.toml`.

FCStd is a ZIP of XML plus embedded OCC B-Rep and application side entries. The
XML layers have no byte layout, but four side entries do, and those are tabled
here: the mesh-kernel record, the point-kernel record, the packed-colour list,
and the link-array element list (§11, §9).

The §11 side entries state that both integer byte orders are accepted for the
mesh record when its magic and version agree; every other stated width in these
records is little-endian.

## `mesh_kernel_side_entry_header`

Spec §11 · layout: byte offsets · size: 264 B

Both integer byte orders are accepted when the magic and version agree, so the header states no single endianness. Two 32-bit counts follow at +264, then ordered float32 XYZ points and facets, then six float32 bounding-box limits.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `magic` | `u32` | unstated | spec | The spec states outright that both byte orders are accepted for this word. |
| 4 | 4 | `version` | `u32` | unstated | spec | The spec states outright that both byte orders are accepted for this word. |
| 8 | 256 | `information` | `bytes[256]` | unstated | spec | and a 256-byte information field |

Cross-checked against code:

- `crates/cadmpeg-codec-freecad/src/application_geometry.rs` — The parser's mesh magic matches offset 0.
- `crates/cadmpeg-codec-freecad/src/application_geometry.rs` — The parser's mesh version matches offset 4.
- `crates/cadmpeg-codec-freecad/src/application_geometry.rs` — The parser skips the stated 256-byte information field.

## `mesh_facet`

Spec §11 · layout: byte offsets · size: 24 B

Six 32-bit indices. The 24-byte stride is derived from the stated field count and the record's 32-bit index width; the spec states no total.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 12 | `point_indices` | `u32[3]` | little | spec | Each facet contains three zero-based point indices |
| 12 | 12 | `neighbour_indices` | `u32[3]` | little | derived | Offset derived from the three preceding 32-bit indices. |

Cross-checked against code:

- `crates/cadmpeg-codec-freecad/src/application_geometry.rs` — The parser bounds the facet array at 24 bytes per facet, matching the derived stride.

## `point_kernel_side_entry_header`

Spec §11 · layout: byte offsets · size: 4 B

Fixed prefix only; `count` float32 XYZ triples follow at +4. The property's `Points` element carries the sixteen finite row-major transform scalars separately, in XML.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `point_count` | `u32` | little | spec | a little-endian 32-bit point count followed by ordered float32 XYZ triples |

## `packed_color_list_header`

Spec §11 · layout: byte offsets · size: 4 B

Applies to `DiffuseColor`, `LineColorArray`, and `PointColorArray`. A count of one applies its colour to every member of the corresponding element-map group; otherwise the count must equal the number of names in that ordered group.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `count` | `u32` | little | spec | a little-endian count followed by packed-color records |

Cross-checked against code:

- `crates/cadmpeg-codec-freecad/src/gui.rs` — The parser bounds each packed-colour record at 4 bytes after the little-endian count prefix.

## `link_array_side_entry_header`

Spec §9 · layout: byte offsets · size: 4 B

Fixed prefix only. Placement records carry position plus quaternion (seven components); scale records carry three. The exact entry length selects f32 or f64 components, so no fixed element stride exists.

| Offset | Size | Field | Type | Endian | Src | Meaning |
| -----: | ---: | ----- | ---- | ------ | --- | ------- |
| 0 | 4 | `element_count` | `u32` | little | spec | Each side entry begins with a little-endian element count |

Cross-checked against code:

- `crates/cadmpeg-codec-freecad/src/product.rs` — The parser matches entry length against a 4-byte count prefix plus 8-byte or 4-byte components.

## Not tabulated

| Area | Spec | Reason |
| ---- | ---- | ------ |
| `Document.xml` and `GuiDocument.xml` element and attribute schema | §2 | XML, not a byte or column layout. The specification names about ten element and attribute identifiers in prose and tabulates none; the real schema exists only as string literals in the parser (26 tag names, 45 attribute names, with case-sensitive near-duplicates such as `value`/`Value`). |
| Binary OCC B-Rep shape sets | §7 | The specification states the accepted header band (text V1-V3, binary V1-V4) and which behaviours the version gates, but no record layout. The binary body is a nested, count-driven table stream with a 256-level depth cap. |
| Element-map and string-table streams | §6 | Text streams framed by magic strings and newline counts, with hexadecimal ids and delta encoding. Positional in the line sense, not in the byte or column sense. |
| ZIP container framing | §2 | The outer envelope is a standard ZIP archive. Its local-header layout is the external ZIP specification, not a cadmpeg finding. |
