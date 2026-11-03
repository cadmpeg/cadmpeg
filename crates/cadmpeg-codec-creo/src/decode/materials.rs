// SPDX-License-Identifier: Apache-2.0
//! Material and appearance tables in `NeuPrtSld` sections.
//!
//! `color_tab` holds a prototype color row plus positional color rows; each
//! mapped row projects one neutral appearance. `element_colors` binds faces
//! to color indices by the row's slot identity. Only lane-token pairs the
//! grammar maps become values; the rest stays in the retained section bytes.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{Appearance, AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::ids::{AppearanceBindingId, AppearanceId};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{Color, Face, Sense};

use crate::container::{self, ContainerScan};
use crate::psb;
use crate::scalar::{self, ScalarCache, StatefulScalarKind};

/// Mapped scalars per color row: `color_rgb[3]`, ambient, diffuse, specular,
/// `spec_coef`, transmission, `highlight[3]`, reflection.
const COLOR_ROW_SLOTS: usize = 12;

/// Tail slots with no neutral mapping: bump, decal, texture.
const COLOR_TAIL_SLOTS: usize = 3;

/// File-constant back-side markers for `color_inds[1]`.
const BACK_MARKERS: [u8; 2] = [0xf6, 0xe4];

/// One mapped color row: twelve lane scalars in grammar order.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ColorRow {
    /// `color_rgb` triple.
    pub(super) rgb: [f64; 3],
    /// Ambient scalar.
    pub(super) ambient: f64,
    /// Diffuse scalar.
    pub(super) diffuse: f64,
    /// Specular scalar.
    pub(super) specular: f64,
    /// Specular-coefficient scalar.
    pub(super) spec_coef: f64,
    /// Transmission scalar; alpha is one minus transmission.
    pub(super) transmission: f64,
    /// `highlight` triple.
    pub(super) highlight: [f64; 3],
    /// Reflection scalar.
    pub(super) reflection: f64,
}

/// One mapped element row: a face bound to a color index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ElementRow {
    /// Face identity joining `crv_array` face identities.
    pub(super) face_id: u32,
    /// Color index selecting the side along the carrier normal.
    pub(super) color_index: u32,
}

/// Decode one scalar lane run of exactly `slots` values.
fn run_scalars(body: &[u8], cache: &ScalarCache, slots: usize) -> Option<(Vec<f64>, usize)> {
    let mut values = Vec::new();
    let mut cursor = 0;
    let mut previous = None;
    while values.len() < slots {
        let (value, repeat, end) = scalar::decode_stateful_scalar_run(
            body,
            cursor,
            previous,
            cache,
            StatefulScalarKind::Plane,
        )?;
        if repeat == 0 || end <= cursor || end > body.len() {
            return None;
        }
        for _ in 0..repeat {
            values.push(value);
        }
        previous = Some(value);
        cursor = end;
    }
    (values.len() == slots).then_some((values, cursor))
}

/// Find a PSB label `e0 <token> <name>\0` and return the byte after the name.
fn label_end(bytes: &[u8], name: &[u8]) -> Option<usize> {
    let mut from = 0;
    while let Some(found) = cadmpeg_core::bytes::find_in(bytes, name, from, bytes.len()) {
        let end = found + name.len();
        if found >= 2 && bytes[found - 2] == 0xe0 && bytes[end - 1] == 0 {
            return Some(end);
        }
        from = found + 1;
    }
    None
}

/// Read a counted `f8 <count>` opener and return the count and the end offset.
fn counted_opener(bytes: &[u8], cursor: usize) -> Option<(u32, usize)> {
    if bytes.get(cursor) != Some(&0xf8) {
        return None;
    }
    psb::complete_compact_int(bytes, cursor + 1)
}

/// Decode one named prototype body: `e1` is null, `f8 <count>` runs that many
/// scalars, anything else runs one scalar.
fn named_scalars(bytes: &[u8], cursor: usize, cache: &ScalarCache) -> Option<(Vec<f64>, usize)> {
    if bytes.get(cursor) == Some(&0xe1) {
        return Some((Vec::new(), cursor + 1));
    }
    if bytes.get(cursor) == Some(&0xf8) {
        let (count, after) = counted_opener(bytes, cursor)?;
        let slots = usize::try_from(count).ok()?;
        let (values, end) = run_scalars(&bytes[after..], cache, slots)?;
        return Some((values, after + end));
    }
    let (values, end) = run_scalars(&bytes[cursor..], cache, 1)?;
    Some((values, cursor + end))
}

/// One named prototype field and the byte after its body.
fn named_field<'a>(
    bytes: &'a [u8],
    cursor: usize,
    cache: &ScalarCache,
) -> Option<(&'a [u8], Vec<f64>, usize)> {
    if bytes.get(cursor) != Some(&0xe0) {
        return None;
    }
    let name_start = cursor + 2;
    let name_end = bytes[name_start..].iter().position(|byte| *byte == 0)?;
    let name = &bytes[name_start..name_start + name_end];
    let (values, end) = named_scalars(bytes, name_start + name_end + 1, cache)?;
    Some((name, values, end))
}

/// Build a color row from its twelve mapped scalars.
fn color_row(values: &[f64]) -> Option<ColorRow> {
    let [r0, r1, r2, ambient, diffuse, specular, spec_coef, transmission, h0, h1, h2, reflection] =
        *values
    else {
        return None;
    };
    Some(ColorRow {
        rgb: [r0, r1, r2],
        ambient,
        diffuse,
        specular,
        spec_coef,
        transmission,
        highlight: [h0, h1, h2],
        reflection,
    })
}

/// Expected named fields of the `color_tab` prototype with no mapping.
const COLOR_TAIL_FIELDS: [&[u8]; 3] = [b"bump", b"decal", b"texture"];

/// Parse the `color_tab` prototype named fields and return the mapped row.
fn prototype_color_row(
    bytes: &[u8],
    cursor: usize,
    cache: &ScalarCache,
) -> Option<(ColorRow, usize)> {
    let mut fields = BTreeMap::<Vec<u8>, Vec<f64>>::new();
    let mut at = cursor;
    while let Some((name, values, end)) = named_field(bytes, at, cache) {
        if fields.insert(name.to_vec(), values).is_some() {
            return None;
        }
        at = end;
    }
    let get = |name: &[u8], slots: usize| {
        fields
            .get(name)
            .filter(|values| values.len() == slots)
            .cloned()
    };
    let rgb = get(b"color_rgb", 3)?;
    let highlight = get(b"highlight", 3)?;
    let one = |name: &[u8]| get(name, 1).and_then(|values| values.into_iter().next());
    let row = color_row(
        &[
            rgb[0],
            rgb[1],
            rgb[2],
            one(b"ambient")?,
            one(b"diffuse")?,
            one(b"specular")?,
            one(b"spec_coef")?,
            one(b"transmission")?,
            highlight[0],
            highlight[1],
            highlight[2],
            one(b"reflection")?,
        ]
        .into_iter()
        .collect::<Vec<_>>(),
    )?;
    for tail in COLOR_TAIL_FIELDS {
        fields.get(tail)?;
    }
    Some((row, at))
}

/// Parse one positional color row: twelve mapped scalars plus three tail
/// slots that stay native.
fn positional_color_row(
    bytes: &[u8],
    cursor: usize,
    cache: &ScalarCache,
) -> Option<(ColorRow, usize)> {
    let (values, end) = run_scalars(&bytes[cursor..], cache, COLOR_ROW_SLOTS)?;
    let mut at = cursor + end;
    for _ in 0..COLOR_TAIL_SLOTS {
        if bytes.get(at) == Some(&0xe1) {
            at += 1;
            continue;
        }
        let (_, end) = run_scalars(&bytes[at..], cache, 1)?;
        at += end;
    }
    Some((color_row(&values)?, at))
}

/// Parse the `color_tab` table: the prototype plus `n - 1` positional rows.
fn color_table(bytes: &[u8], cache: &ScalarCache) -> Option<Vec<ColorRow>> {
    let label = label_end(bytes, b"color_tab\0")?;
    let (count, mut at) = counted_opener(bytes, label)?;
    for token in [0xf7, 0x0a, 0xfb, 0xe2] {
        if bytes.get(at) != Some(&token) {
            return None;
        }
        at += 1;
    }
    if count == 0 {
        return Some(Vec::new());
    }
    let (prototype, end) = prototype_color_row(bytes, at, cache)?;
    at = end;
    let mut rows = vec![prototype];
    if count == 1 {
        return Some(rows);
    }
    for token in [0xf1, 0xf7, 0x0a, 0xe2] {
        if bytes.get(at) != Some(&token) {
            return None;
        }
        at += 1;
    }
    let remaining = usize::try_from(count - 1).ok()?;
    for index in 0..remaining {
        let (row, end) = positional_color_row(bytes, at, cache)?;
        rows.push(row);
        at = end;
        if index + 1 < remaining {
            if bytes.get(at) != Some(&0xe2) {
                return None;
            }
            at += 1;
        }
    }
    Some(rows)
}

/// Parse the `element_colors` prototype and return the byte after its close.
fn element_prototype(bytes: &[u8], mut at: usize) -> Option<usize> {
    let mut seen_id = false;
    let mut seen_inds = false;
    let mut seen_pl = false;
    let mut seen_ptr = false;
    loop {
        if bytes.get(at) != Some(&0xe0) {
            return None;
        }
        let name_start = at + 2;
        let name_end = bytes[name_start..].iter().position(|byte| *byte == 0)?;
        let name = &bytes[name_start..name_start + name_end];
        at = name_start + name_end + 1;
        match name {
            n if n == b"element_id" => {
                psb::parent_feature_id(bytes, at)?;
                let (_, end) = psb::parent_feature_id(bytes, at)?;
                at = end;
                seen_id = true;
            }
            n if n == b"color_inds" => {
                let (count, after) = counted_opener(bytes, at)?;
                if count != 2 || bytes.len() < after + 2 {
                    return None;
                }
                at = after + 2;
                seen_inds = true;
            }
            n if n == b"surf_pl" => {
                for token in [0xf8, 0x02, 0xf7, 0x19, 0xfb, 0xe2] {
                    if bytes.get(at) != Some(&token) {
                        return None;
                    }
                    at += 1;
                }
                seen_pl = true;
            }
            n if n == b"surfprop_ptr" => {
                if bytes.get(at) != Some(&0xe1) {
                    return None;
                }
                at += 1;
                seen_ptr = true;
            }
            _ => return None,
        }
        if seen_id && seen_inds && seen_pl && seen_ptr {
            break;
        }
    }
    Some(at)
}

/// Parse one positional element row after its `f3 f7 16 e2` opener.
fn element_row(bytes: &[u8], at: usize, color_count: usize) -> Option<(ElementRow, usize)> {
    let (face_id, after_face) = psb::parent_feature_id(bytes, at)?;
    let (index, after_index) = psb::complete_compact_int(bytes, after_face)?;
    let marker = *bytes.get(after_index)?;
    if !BACK_MARKERS.contains(&marker) {
        return None;
    }
    let mut at = after_index + 1;
    for token in [0xf8, 0x02, 0xf7, 0x19, 0xfb, 0xe2, 0xf7, 0x1a, 0xe1] {
        if bytes.get(at) != Some(&token) {
            return None;
        }
        at += 1;
    }
    if usize::try_from(index).ok()? >= color_count {
        return None;
    }
    Some((
        ElementRow {
            face_id,
            color_index: index,
        },
        at,
    ))
}

/// Parse the `element_colors` table.
fn element_table(bytes: &[u8], color_count: usize) -> Option<Vec<ElementRow>> {
    let label = label_end(bytes, b"element_colors\0")?;
    if bytes.get(label) != Some(&0xf2) {
        return None;
    }
    let (count, mut at) = counted_opener(bytes, label + 1)?;
    for token in [0xf7, 0x16, 0xfb, 0xe2] {
        if bytes.get(at) != Some(&token) {
            return None;
        }
        at += 1;
    }
    at = element_prototype(bytes, at)?;
    for token in [0xf1, 0xf7, 0x19, 0xe2] {
        if bytes.get(at) != Some(&token) {
            return None;
        }
        at += 1;
    }
    let mut rows = Vec::new();
    let remaining = count.checked_sub(1)?;
    for _ in 0..remaining {
        for token in [0xe1, 0xf3, 0xf7, 0x16, 0xe2] {
            if bytes.get(at) != Some(&token) {
                return None;
            }
            at += 1;
        }
        let (row, end) = element_row(bytes, at, color_count)?;
        rows.push(row);
        at = end;
        for token in [0xf1, 0xf7, 0x19, 0xe2] {
            if bytes.get(at) != Some(&token) {
                return None;
            }
            at += 1;
        }
    }
    if bytes.get(at) != Some(&0xe1) {
        return None;
    }
    Some(rows)
}

/// Property keys for the mapped non-color scalars, in grammar order.
const COLOR_PROPERTIES: [(&str, fn(&ColorRow) -> f64); 5] = [
    ("ambient", |row| row.ambient),
    ("diffuse", |row| row.diffuse),
    ("specular", |row| row.specular),
    ("spec_coef", |row| row.spec_coef),
    ("reflection", |row| row.reflection),
];

/// Admit one appearance per used color index.
fn admit_appearances(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    colors: &[ColorRow],
    used: &BTreeSet<u32>,
    native_ref: &str,
    section_offset: usize,
) -> Result<BTreeMap<u32, AppearanceId>, CodecError> {
    let mut appearances = BTreeMap::new();
    for index in used {
        let Ok(offset) = usize::try_from(*index) else {
            continue;
        };
        let Some(row) = colors.get(offset) else {
            continue;
        };
        if !row.rgb.iter().all(|value| value.is_finite()) {
            continue;
        }
        let [r, g, b] = row.rgb.map(|value| value as f32);
        let alpha = 1.0 - row.transmission as f32;
        let Some(base_color) = Color::new(r, g, b, alpha) else {
            continue;
        };
        let id = AppearanceId::mint(format!(
            "creo:document:appearance#neu-{section_offset}-{index}"
        ))
        .map_err(CodecError::malformed)?;
        if ir
            .model
            .appearances
            .iter()
            .any(|appearance| appearance.id == id)
        {
            appearances.insert(*index, id);
            continue;
        }
        let mut properties = BTreeMap::new();
        for (key, select) in COLOR_PROPERTIES {
            let Some(value) = FiniteReal::new(select(row)) else {
                continue;
            };
            let Some(key) = cadmpeg_core::text::NonBlankString::new(key) else {
                continue;
            };
            properties.insert(key, value);
        }
        ctx.charge_entities(1, "admit Creo appearance")?;
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(native_ref.len()),
            "Creo appearance native link",
        )?;
        ir.model.appearances.push(Appearance {
            id: id.clone(),
            native_ref: Some(native_ref.to_owned()),
            name: None,
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: Some(base_color),
            properties,
            textures: Vec::new(),
        });
        appearances.insert(*index, id);
    }
    Ok(appearances)
}

/// Whether the face carries boundary loops.
fn face_is_bounded(face: &Face) -> bool {
    face.loops.iter().next().is_some()
}

/// Bind every bounded face on a matched carrier, with the side chosen by
/// face orientation: a forward face shows the side along the carrier normal
/// and inherits the row's color; a reversed face shows the opposite side,
/// whose marker stays native.
fn admit_bindings(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    elements: &[ElementRow],
    appearances: &BTreeMap<u32, AppearanceId>,
    native_ref: &str,
    section_offset: usize,
) -> Result<(), CodecError> {
    for element in elements {
        let Some(appearance) = appearances.get(&element.color_index) else {
            continue;
        };
        let surface = format!("creo:visibgeom:surface#{}", element.face_id);
        let faces = ir
            .model
            .faces
            .iter()
            .filter(|face| face.surface.as_str() == surface && face_is_bounded(face))
            .map(|face| (face.id.clone(), face.sense))
            .collect::<Vec<_>>();
        for (face, sense) in faces {
            if sense != Sense::Forward {
                continue;
            }
            let id = AppearanceBindingId::mint(format!(
                "creo:document:appearance-binding#{section_offset}-{}",
                element.face_id
            ))
            .map_err(CodecError::malformed)?;
            if ir
                .model
                .appearance_bindings
                .iter()
                .any(|binding| binding.id == id)
            {
                continue;
            }
            ctx.charge_entities(1, "admit Creo appearance binding")?;
            ctx.retain_bytes(
                cadmpeg_core::decode::u64_from_index(native_ref.len()),
                "Creo appearance binding native link",
            )?;
            ir.model.appearance_bindings.push(AppearanceBinding {
                id,
                native_ref: Some(native_ref.to_owned()),
                target: AppearanceTarget::Face(face),
                appearance: appearance.clone(),
                source_entity_id: None,
                object_type: None,
                visible: None,
                channels: BTreeMap::new(),
            });
        }
    }
    Ok(())
}

/// Project appearances and face bindings from one material section.
fn project_section(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    bytes: &[u8],
    section: &crate::container::Section,
) -> Result<(), CodecError> {
    let section_offset = section.offset();
    let cache = ScalarCache::from_section_checked(bytes);
    let Some(colors) = color_table(bytes, &cache) else {
        return Ok(());
    };
    if colors.is_empty() {
        return Ok(());
    }
    let Some(elements) = element_table(bytes, colors.len()) else {
        return Ok(());
    };
    let used = elements
        .iter()
        .map(|element| element.color_index)
        .collect::<BTreeSet<_>>();
    if used.is_empty() {
        return Ok(());
    }
    let namespace = crate::identity::section_namespace(section.name())
        .ok_or_else(|| CodecError::malformed("material section without a namespace"))?;
    let native_ref =
        crate::identity::compose_checked::<cadmpeg_ir::ids::UnknownId>(&namespace, section_offset)
            .map(|id| id.as_str().to_owned())?;
    let appearances = admit_appearances(ctx, ir, &colors, &used, &native_ref, section_offset)?;
    admit_bindings(
        ctx,
        ir,
        &elements,
        &appearances,
        &native_ref,
        section_offset,
    )
}

/// Project appearances and face bindings from every material section.
///
/// Sections without mapped tables keep their retained bytes and gain no
/// neutral records.
pub(super) fn project(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    ir: &mut CadIr,
) -> Result<(), CodecError> {
    for section in &scan.framing.sections {
        if !matches!(section.name(), "NeuPrtSld" | "NeuAsmSld") {
            continue;
        }
        let bytes = container::expanded_section_for(scan, section)
            .map(|expanded| expanded.data.as_ref())
            .or_else(|| container::section_region(&scan.framing.data, section));
        let Some(bytes) = bytes else {
            continue;
        };
        project_section(ctx, ir, bytes, section)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_for(bytes: &[u8]) -> ScalarCache {
        ScalarCache::from_section_checked(bytes)
    }

    fn color_section() -> Vec<u8> {
        let mut bytes = b"\xe0\x00color_tab\0\xf8\x02\xf7\x0a\xfb\xe2".to_vec();
        bytes.extend_from_slice(b"\xe0\x02color_rgb\0\xf8\x03\x18\x18\x0f");
        bytes.extend_from_slice(b"\xe0\x02ambient\0\x29\xe3\x33");
        bytes.extend_from_slice(b"\xe0\x02diffuse\0\x0f");
        bytes.extend_from_slice(b"\xe0\x02specular\0\x0f");
        bytes.extend_from_slice(b"\xe0\x02spec_coef\0\x0f");
        bytes.extend_from_slice(b"\xe0\x02transmission\0\x18");
        bytes.extend_from_slice(b"\xe0\x02highlight\0\xf8\x03\x0f\xe5");
        bytes.extend_from_slice(b"\xe0\x02reflection\0\x18");
        bytes.extend_from_slice(b"\xe0\x00bump\0\xe1");
        bytes.extend_from_slice(b"\xe0\x00decal\0\xe1");
        bytes.extend_from_slice(b"\xe0\x00texture\0\xe1");
        bytes.extend_from_slice(&[0xf1, 0xf7, 0x0a, 0xe2]);
        bytes.extend_from_slice(&[
            0x0f, 0xe4, 0x18, 0x29, 0xe1, 0x99, 0x0f, 0x0f, 0x0f, 0x18, 0x0f, 0xe5, 0x18, 0xe1,
            0xe1, 0xe1,
        ]);
        bytes
    }

    fn element_section() -> Vec<u8> {
        let mut bytes = b"\xe0\x00element_colors\0\xf2\xf8\x03\xf7\x16\xfb\xe2".to_vec();
        bytes.extend_from_slice(b"\xe0\x01element_id\0\x90\x45");
        bytes.extend_from_slice(b"\xe0\x01color_inds\0\xf8\x02\x00\xe4");
        bytes.extend_from_slice(b"\xe0\x00surf_pl\0\xf8\x02\xf7\x19\xfb\xe2");
        bytes.extend_from_slice(b"\xe0\x00surfprop_ptr\0\xe1");
        bytes.extend_from_slice(&[0xf1, 0xf7, 0x19, 0xe2]);
        for (face, index, marker) in [(0x01, 0x00, 0xf6), (0x02, 0x01, 0xe4)] {
            bytes.extend_from_slice(&[0xe1, 0xf3, 0xf7, 0x16, 0xe2]);
            bytes.extend_from_slice(&[face, index, marker]);
            bytes.extend_from_slice(&[0xf8, 0x02, 0xf7, 0x19, 0xfb, 0xe2, 0xf7, 0x1a, 0xe1]);
            bytes.extend_from_slice(&[0xf1, 0xf7, 0x19, 0xe2]);
        }
        bytes.push(0xe1);
        bytes
    }

    #[test]
    fn color_table_parses_prototype_and_positional_row() {
        let bytes = color_section();
        let cache = cache_for(&bytes);
        let rows = color_table(&bytes, &cache).expect("color rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].rgb, [0.0, 0.0, 1.0]);
        assert!((rows[0].ambient - 0.6).abs() < 1e-12);
        assert_eq!(rows[0].highlight, [1.0, 1.0, 1.0]);
        assert_eq!(rows[1].rgb, [1.0, 1.0, 0.0]);
        assert!((rows[1].ambient - 0.55).abs() < 1e-9);
        assert_eq!(rows[1].transmission, 0.0);
    }

    #[test]
    fn element_table_parses_faces_and_indices() {
        let bytes = element_section();
        let rows = element_table(&bytes, 2).expect("element rows");
        assert_eq!(
            rows,
            vec![
                ElementRow {
                    face_id: 1,
                    color_index: 0
                },
                ElementRow {
                    face_id: 2,
                    color_index: 1
                },
            ]
        );
    }

    #[test]
    fn truncated_color_table_yields_no_rows() {
        let bytes = color_section();
        let cut = &bytes[..bytes.len() - 4];
        let cache = cache_for(cut);
        assert!(color_table(cut, &cache).is_none());
    }

    #[test]
    fn color_index_past_the_table_is_not_a_match() {
        let bytes = element_section();
        assert!(element_table(&bytes, 1).is_none());
    }

    #[test]
    fn unknown_back_marker_is_not_a_match() {
        let mut bytes = element_section();
        let position = bytes
            .windows(3)
            .position(|window| window == [0x01, 0x00, 0xf6])
            .expect("first row marker");
        bytes[position + 2] = 0x00;
        assert!(element_table(&bytes, 2).is_none());
    }
}
