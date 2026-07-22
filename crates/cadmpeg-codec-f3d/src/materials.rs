// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion `.protein` appearance assets and bind them to B-rep bodies.
//!
//! Material and appearance semantics are defined in [spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials).
//! [`decode`] reads appearance records without resolving body bindings.
//! [`decode_with_bodies`] joins Protein assets, Design assignments, ACT
//! channels, and ASM body keys through the design-entity join backbone in
//! [spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials).

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use crate::records::DesignMaterialAssignment;
use cadmpeg_ir::appearance::{
    Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::ids::{AppearanceId, BodyId};
use cadmpeg_ir::le::{u32_at, u64_at};
use cadmpeg_ir::topology::Color;

use crate::bytes::{
    is_guid_prefix, lp_ascii_filtered, lp_utf16_bounded, lp_utf16_bytes, take_lp_utf8,
};
use crate::container::{role, ContainerScan};

const PAGE_SIZE: usize = 0x88;
const RECORD_MARKER: &[u8] = b"\x80\x00\x01\x00";

/// Compare a serialized Protein visual token with a Design visual GUID.
///
/// Protein assets may append one or more `_Post2015` revisions to their
/// 36-character GUID. Design assignments retain only the GUID prefix.
pub(crate) fn visual_guid_matches(left: &str, right: &str) -> bool {
    is_guid_prefix(left) && is_guid_prefix(right) && left[..36].eq_ignore_ascii_case(&right[..36])
}

pub(crate) fn encode_protein(appearance: &Appearance) -> Result<Vec<u8>, CodecError> {
    if !appearance.textures.is_empty() {
        return Err(CodecError::NotImplemented(
            "source-less F3D cannot synthesize connected Protein texture assets".into(),
        ));
    }
    let schema = appearance.schema.as_deref().unwrap_or("GenericSchema");
    let guid = appearance
        .visual_guid
        .as_deref()
        .or(appearance.asset_guid.as_deref())
        .ok_or_else(|| {
            CodecError::Malformed("source-less appearance lacks an asset GUID".into())
        })?;
    let name = appearance.name.as_deref().unwrap_or("Prism-001");
    let mut logical = RECORD_MARKER.to_vec();
    for value in [schema, guid, name, "00000000-0000-0000-0000-000000000000"] {
        push_lp(&mut logical, value)?;
    }
    let value_block = logical.len();
    match schema {
        "GenericSchema" => {
            logical.resize(value_block + 209, 0);
            write_color(&mut logical, value_block + 112, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("reflectivity_at_0deg") {
                logical[value_block + 171..value_block + 175].copy_from_slice(b"\x0c\x00\x00\x00");
                logical[value_block + 175..value_block + 183].copy_from_slice(&value.to_le_bytes());
            }
            if let Some(value) = appearance.properties.get("refraction_index") {
                logical[value_block + 197..value_block + 201].copy_from_slice(b"\x0c\x00\x00\x00");
                logical[value_block + 201..value_block + 209].copy_from_slice(&value.to_le_bytes());
            }
        }
        "PrismOpaqueSchema" | "PrismMetalSchema" => {
            logical.resize(value_block + 96, 0);
            write_color(&mut logical, value_block + 8, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("surface_roughness") {
                logical[value_block + 64..value_block + 68].copy_from_slice(b"\x0e\x20\x00\x00");
                logical[value_block + 68..value_block + 76].copy_from_slice(&value.to_le_bytes());
            }
        }
        "PrismTransparentSchema" => {
            logical.resize(value_block + 177, 0);
            write_color(&mut logical, value_block + 121, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("refraction_index") {
                logical[value_block + 169..value_block + 177].copy_from_slice(&value.to_le_bytes());
            }
        }
        "PhysMatSchema"
        | "StructuralMetalSchema"
        | "StructuralPlasticSchema"
        | "ThermalSolidSchema" => logical.resize(value_block + 8, 0),
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "source-less Protein schema {schema} is unsupported"
            )))
        }
    }
    let instance = page_logical(&logical)?;
    let mut catalog = RECORD_MARKER.to_vec();
    for value in [
        schema,
        name,
        "Default",
        appearance.category.as_deref().unwrap_or("Generated"),
    ] {
        push_lp(&mut catalog, value)?;
    }
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("AssetData/InstanceProperties.bin", options)
        .map_err(|error| {
            CodecError::Malformed(format!("cannot create Protein instance: {error}"))
        })?;
    zip.write_all(&instance)?;
    zip.start_file("AssetData/DefinitionIteratorProperties.bin", options)
        .map_err(|error| {
            CodecError::Malformed(format!("cannot create Protein catalog: {error}"))
        })?;
    zip.write_all(&catalog)?;
    Ok(zip
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish Protein asset: {error}")))?
        .into_inner())
}

fn push_lp(out: &mut Vec<u8>, value: &str) -> Result<(), CodecError> {
    let length = u32::try_from(value.len())
        .map_err(|_| CodecError::Malformed("Protein string exceeds u32::MAX".into()))?;
    out.extend_from_slice(&length.to_le_bytes());
    out.extend_from_slice(value.as_bytes());
    Ok(())
}

fn write_color(out: &mut [u8], offset: usize, color: Option<Color>) -> Result<(), CodecError> {
    let color = color.ok_or_else(|| {
        CodecError::Malformed("visual source-less Protein appearance lacks base_color".into())
    })?;
    for (ordinal, value) in [color.r, color.g, color.b, color.a].into_iter().enumerate() {
        if !value.is_finite() {
            return Err(CodecError::Malformed(
                "Protein base color must contain finite channels".into(),
            ));
        }
        let at = offset + ordinal * 8;
        out[at..at + 8].copy_from_slice(&f64::from(value).to_le_bytes());
    }
    Ok(())
}

fn page_logical(logical: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&(PAGE_SIZE as u32).to_le_bytes());
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let first = logical.len().min(PAGE_SIZE - 4);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    bytes.resize(16 + PAGE_SIZE, 0);
    let mut rest = &logical[first..];
    while rest.len() > PAGE_SIZE - 8 {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(b"\x80\x00\x00\x00");
        bytes.extend_from_slice(&rest[..PAGE_SIZE - 8]);
        rest = &rest[PAGE_SIZE - 8..];
    }
    if !rest.is_empty() {
        bytes.extend_from_slice(&[0xff; 4]);
        let length = u16::try_from(rest.len())
            .map_err(|_| CodecError::Malformed("Protein tail page exceeds u16::MAX".into()))?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        let end = 16 + (bytes.len() - 16).next_multiple_of(PAGE_SIZE);
        bytes.resize(end, 0);
    }
    Ok(bytes)
}

#[derive(Default)]
pub(crate) struct ProteinAppearanceEdit {
    pub(crate) color: Option<Color>,
    pub(crate) properties: BTreeMap<String, f64>,
}

pub(crate) fn patch_protein_appearances(
    protein: &[u8],
    edits: &BTreeMap<String, ProteinAppearanceEdit>,
) -> Result<(Vec<u8>, std::collections::BTreeSet<String>), CodecError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(protein)).map_err(|error| {
        CodecError::Malformed(format!("cannot open nested Protein ZIP: {error}"))
    })?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut patched = std::collections::BTreeSet::new();
    let mut total_inflated = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            CodecError::Malformed(format!("cannot read nested Protein entry: {error}"))
        })?;
        let name = entry.name().to_owned();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let declared_size = entry.size();
        total_inflated = total_inflated.checked_add(declared_size).ok_or_else(|| {
            CodecError::Malformed("Protein ZIP total inflated size overflows u64".into())
        })?;
        if total_inflated > crate::container::MAX_ARCHIVE_BYTES {
            return Err(CodecError::Malformed(format!(
                "Protein ZIP entries declare {total_inflated} inflated bytes; total limit is {}",
                crate::container::MAX_ARCHIVE_BYTES
            )));
        }
        let mut bytes = crate::container::read_entry_bounded(&mut entry, declared_size, &name)?;
        if name.ends_with("AssetData/InstanceProperties.bin") {
            patch_instance_colors(&mut bytes, edits, &mut patched)?;
        }
        zip.start_file(name, options).map_err(|error| {
            CodecError::Malformed(format!("cannot write nested Protein entry: {error}"))
        })?;
        zip.write_all(&bytes)?;
    }
    let bytes = zip
        .finish()
        .map_err(|error| CodecError::Malformed(format!("cannot finish Protein ZIP: {error}")))?
        .into_inner();
    Ok((bytes, patched))
}

fn patch_instance_colors(
    bytes: &mut [u8],
    edits: &BTreeMap<String, ProteinAppearanceEdit>,
    patched: &mut std::collections::BTreeSet<String>,
) -> Result<(), CodecError> {
    let logical = dechunk(bytes).ok_or_else(|| {
        CodecError::Malformed("cannot map Protein InstanceProperties pages".into())
    })?;
    let starts = logical
        .windows(RECORD_MARKER.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == RECORD_MARKER).then_some(offset))
        .collect::<Vec<_>>();
    for start in starts {
        let record = &logical[start..];
        let mut position = RECORD_MARKER.len();
        let schema = take_lp_utf8(record, &mut position).ok_or_else(|| {
            CodecError::Malformed("Protein appearance schema is truncated".into())
        })?;
        let guid = take_lp_utf8(record, &mut position)
            .ok_or_else(|| CodecError::Malformed("Protein appearance GUID is truncated".into()))?;
        let _ = take_lp_utf8(record, &mut position);
        let _ = take_lp_utf8(record, &mut position);
        let Some(edit) = edits.get(&guid) else {
            continue;
        };
        let delta = generic_connection_delta(record, position);
        if let Some(color) = edit.color {
            let relative = match schema.as_str() {
                "GenericSchema" => position + 112 + delta,
                "PrismOpaqueSchema" | "PrismMetalSchema" => position + 8,
                "PrismTransparentSchema" => position + 121,
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "Protein schema {schema} has no writable color carrier"
                    )))
                }
            };
            for (ordinal, value) in [color.r, color.g, color.b, color.a].into_iter().enumerate() {
                patch_logical_f64(bytes, start + relative + ordinal * 8, f64::from(value))?;
            }
        }
        for (name, value) in &edit.properties {
            let relative = match (schema.as_str(), name.as_str()) {
                ("GenericSchema", "reflectivity_at_0deg") => position + 175 + delta,
                ("GenericSchema", "refraction_index") => position + 201 + delta,
                ("PrismOpaqueSchema", "surface_roughness") => {
                    find(record, b"\x0e\x20\x00\x00", position)
                        .map(|marker| marker + 4)
                        .ok_or_else(|| {
                            CodecError::Malformed("Protein roughness carrier is absent".into())
                        })?
                }
                ("PrismTransparentSchema", "refraction_index") => position + 169,
                _ => {
                    return Err(CodecError::NotImplemented(format!(
                        "Protein schema {schema} property {name} has no writable carrier"
                    )))
                }
            };
            patch_logical_f64(bytes, start + relative, *value)?;
        }
        patched.insert(guid);
    }
    Ok(())
}

fn patch_logical_f64(
    bytes: &mut [u8],
    logical_offset: usize,
    value: f64,
) -> Result<(), CodecError> {
    for (ordinal, byte) in value.to_le_bytes().into_iter().enumerate() {
        let physical = logical_to_physical(bytes, logical_offset + ordinal).ok_or_else(|| {
            CodecError::Malformed("Protein scalar offset is outside paged storage".into())
        })?;
        bytes[physical] = byte;
    }
    Ok(())
}

fn logical_to_physical(bytes: &[u8], logical_offset: usize) -> Option<usize> {
    let mut logical_start = 0usize;
    for (index, page) in bytes.get(16..)?.chunks_exact(PAGE_SIZE).enumerate() {
        let (physical_in_page, length) = if page.get(4..8) == Some(RECORD_MARKER) {
            (4, PAGE_SIZE - 4)
        } else if page.get(4..8) == Some(b"\x80\x00\x00\x00") {
            (8, PAGE_SIZE - 8)
        } else if page.get(0..4) == Some(b"\xff\xff\xff\xff") {
            (
                8,
                u16::from_le_bytes(page.get(4..6)?.try_into().ok()?) as usize,
            )
        } else {
            return None;
        };
        if logical_offset < logical_start + length {
            return Some(
                16 + index * PAGE_SIZE + physical_in_page + logical_offset - logical_start,
            );
        }
        logical_start += length;
    }
    None
}

/// Appearance assets and body bindings from one material decode.
///
/// Bindings follow the design-entity join backbone in
/// [spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials).
#[derive(Default)]
pub struct DecodedMaterials {
    /// Merged appearance records, deduplicated by [`AppearanceId`].
    pub appearances: Vec<Appearance>,
    /// Body-to-appearance bindings resolved via ACT/design/ASM body-key joins.
    pub bindings: Vec<AppearanceBinding>,
    /// Per-face appearance assignments awaiting the BREP face-attribute join.
    pub face_assignments: Vec<FaceAppearanceAssignment>,
    /// Whether the document serializes any body or face appearance assignment.
    ///
    /// Protein assets form a document-local appearance catalog and need not be
    /// assigned to topology. This distinguishes an unassigned catalog from an
    /// assignment that failed to resolve.
    pub has_topology_assignments: bool,
}

/// Decode `.protein` assets and Design and ACT assignments without ASM body
/// bindings.
///
/// The [spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials)
/// `asm_body_key` join is skipped. Use [`decode_with_bodies`] when ASM body keys
/// are available.
pub fn decode(scan: &ContainerScan) -> Result<DecodedMaterials, CodecError> {
    decode_with_bodies(scan, &std::collections::HashMap::new())
}

/// Decode appearance assets and resolve body bindings through
/// `body_keys` (`BodyId` to the ASM `Body.chunk[1]` value), closing the
/// design-entity join backbone in
/// [spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials).
pub fn decode_with_bodies<S: std::hash::BuildHasher>(
    scan: &ContainerScan,
    body_keys: &std::collections::HashMap<BodyId, u64, S>,
) -> Result<DecodedMaterials, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::PROTEIN)
    {
        let payload = scan.entry_bytes(&entry.name)?;
        let Some(instance) = instance_properties(payload) else {
            continue;
        };
        let Some(logical) = dechunk(&instance) else {
            continue;
        };
        let catalog = definition_catalog(payload);
        let mut appearances = if crate::protein::has_schemas(payload) {
            let records = crate::protein::decode(payload, &logical)?;
            let mut decoded = appearances_from_schema_records(&records);
            let decoded_ids = decoded
                .iter()
                .map(|appearance| appearance.id.clone())
                .collect::<std::collections::HashSet<_>>();
            decoded.extend(
                decode_fixed_logical_records(&logical)
                    .into_iter()
                    .filter(|appearance| !decoded_ids.contains(&appearance.id)),
            );
            decoded
        } else {
            decode_fixed_logical_records(&logical)
        };
        for appearance in &mut appearances {
            if let Some(name) = appearance.name.as_deref() {
                if let Some((schema, category)) = catalog.get(name) {
                    appearance.schema = Some(schema.clone());
                    appearance.category = category.clone();
                }
            }
        }
        out.extend(appearances);
    }
    out.sort_by(|a, b| a.id.0.cmp(&b.id.0));
    out.dedup_by(|a, b| a.id == b.id);
    let assignments = decode_design_assignments(scan)?;
    let act_channels = decode_act_channels(scan)?;
    let object_types = decode_design_object_types(scan)?;
    for assignment in &assignments {
        if !out.iter().any(|appearance| {
            appearance
                .visual_guid
                .as_deref()
                .is_some_and(|guid| visual_guid_matches(guid, &assignment.visual_guid))
                || assignment.visual_preset.as_deref() == appearance.name.as_deref()
        }) {
            out.push(Appearance {
                id: AppearanceId(format!("f3d:design:appearance#{}", assignment.visual_guid)),
                name: assignment.visual_preset.clone(),
                asset_guid: Some(assignment.visual_guid.clone()),
                visual_guid: Some(assignment.visual_guid.clone()),
                physical_token: assignment.physical_token.clone(),
                schema: None,
                category: None,
                base_color: None,
                properties: BTreeMap::new(),
                textures: Vec::new(),
            });
        }
    }
    for appearance in &mut out {
        if let Some(assignment) = assignments.iter().find(|assignment| {
            appearance
                .visual_guid
                .as_deref()
                .is_some_and(|guid| visual_guid_matches(guid, &assignment.visual_guid))
        }) {
            appearance.physical_token = assignment.physical_token.clone();
        }
    }
    let mut bindings = bind_bodies(&out, &assignments, &act_channels, &object_types, body_keys);
    let body_overrides = decode_body_appearance_overrides(scan)?;
    for over in &body_overrides {
        let Some(body) = body_keys
            .iter()
            .find_map(|(body, key)| (*key == over.asm_body_key).then_some(body.clone()))
        else {
            continue;
        };
        if bindings
            .iter()
            .any(|binding| binding.target == AppearanceTarget::Body(body.clone()))
        {
            continue;
        }
        let Some(appearance) = out.iter().find(|appearance| {
            appearance
                .visual_guid
                .as_deref()
                .is_some_and(|guid| visual_guid_matches(guid, &over.visual_guid))
        }) else {
            continue;
        };
        bindings.push(AppearanceBinding {
            id: format!(
                "f3d:appearance:body#{}:{}",
                over.entity_suffix, over.visual_guid
            ),
            target: AppearanceTarget::Body(body),
            appearance: appearance.id.clone(),
            source_entity_id: None,
            object_type: object_types.get(&over.entity_suffix).cloned(),
            channels: act_channels
                .get(&over.entity_suffix)
                .cloned()
                .unwrap_or_default(),
        });
    }
    let face_assignments = decode_face_appearance_assignments(scan)?;
    let has_topology_assignments =
        !assignments.is_empty() || !body_overrides.is_empty() || !face_assignments.is_empty();
    Ok(DecodedMaterials {
        appearances: out,
        bindings,
        face_assignments,
        has_topology_assignments,
    })
}

fn appearances_from_schema_records(records: &[crate::protein::DecodedRecord]) -> Vec<Appearance> {
    let textures = records
        .iter()
        .filter_map(texture_asset)
        .map(|texture| (texture.asset_guid.clone(), texture))
        .collect::<BTreeMap<_, _>>();
    records
        .iter()
        .filter(|record| {
            !matches!(
                record.schema.as_str(),
                "UnifiedBitmapSchema" | "BumpMapSchema"
            )
        })
        .map(|record| {
            let mut properties = BTreeMap::new();
            let mut connected = Vec::new();
            for (id, property) in &record.properties {
                if let crate::protein::PropertyValue::Float(value) = property.value {
                    properties.insert(neutral_property_name(id).to_owned(), value);
                }
                for guid in &property.connections {
                    if let Some(texture) = textures.get(guid) {
                        let mut texture = texture.clone();
                        texture.slot.clone_from(id);
                        connected.push(texture);
                    }
                }
            }
            connected.sort_by(|left, right| {
                left.slot
                    .cmp(&right.slot)
                    .then_with(|| left.asset_guid.cmp(&right.asset_guid))
            });
            let base_color = [
                "generic_diffuse",
                "opaque_albedo",
                "surface_albedo",
                "common_Tint_color",
            ]
            .into_iter()
            .find_map(|id| color_property(record, id));
            Appearance {
                id: AppearanceId(format!("f3d:design:appearance#{}", record.guid)),
                name: Some(record.base.clone()),
                asset_guid: Some(record.guid.clone()),
                visual_guid: (!is_physical_schema(&record.schema)).then(|| record.guid.clone()),
                physical_token: None,
                schema: Some(record.schema.clone()),
                category: None,
                base_color,
                properties,
                textures: connected,
            }
        })
        .collect()
}

fn color_property(record: &crate::protein::DecodedRecord, id: &str) -> Option<Color> {
    let crate::protein::PropertyValue::Color([r, g, b, a]) =
        record.properties.get(id).map(|property| &property.value)?
    else {
        return None;
    };
    decoded_color([*r, *g, *b, *a])
}

fn decoded_color(values: [f64; 4]) -> Option<Color> {
    values
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .then_some(Color {
            r: values[0] as f32,
            g: values[1] as f32,
            b: values[2] as f32,
            a: values[3] as f32,
        })
}

fn texture_asset(record: &crate::protein::DecodedRecord) -> Option<TextureRef> {
    if !matches!(
        record.schema.as_str(),
        "UnifiedBitmapSchema" | "BumpMapSchema"
    ) {
        return None;
    }
    let paths = record
        .properties
        .iter()
        .find_map(|(id, property)| {
            (id.ends_with("_Bitmap"))
                .then_some(&property.value)
                .and_then(|value| match value {
                    crate::protein::PropertyValue::TextureUri(paths) => Some(paths.clone()),
                    _ => None,
                })
        })
        .unwrap_or_default();
    let urn = record.properties.iter().find_map(|(id, property)| {
        (id.ends_with("_Bitmap_urn"))
            .then_some(&property.value)
            .and_then(|value| match value {
                crate::protein::PropertyValue::String(value) if !value.is_empty() => {
                    Some(value.clone())
                }
                _ => None,
            })
    });
    let mapping = TextureMap2d {
        map_channel: integer_property(record, "MapChannel").unwrap_or(1),
        uvw_source: integer_property(record, "MapChannel_UVWSource_Advanced").unwrap_or(0),
        u_offset: float_property(record, "UOffset").unwrap_or(0.0),
        v_offset: float_property(record, "VOffset").unwrap_or(0.0),
        u_scale: float_property(record, "UScale").unwrap_or(1.0),
        v_scale: float_property(record, "VScale").unwrap_or(1.0),
        rotation: float_property(record, "WAngle").unwrap_or(0.0).to_radians(),
        repeat_u: boolean_property(record, "URepeat").unwrap_or(true),
        repeat_v: boolean_property(record, "VRepeat").unwrap_or(true),
        real_world_offset_x: distance_property(record, "RealWorldOffsetX").unwrap_or(0.0),
        real_world_offset_y: distance_property(record, "RealWorldOffsetY").unwrap_or(0.0),
        real_world_scale_x: distance_property(record, "RealWorldScaleX").unwrap_or(0.0),
        real_world_scale_y: distance_property(record, "RealWorldScaleY").unwrap_or(0.0),
    };
    let bump = (record.schema == "BumpMapSchema").then(|| BumpMap {
        normal_map: integer_property(record, "bumpmap_Type") == Some(1),
        depth: distance_property(record, "bumpmap_Depth").unwrap_or(0.0),
        normal_scale: float_property(record, "bumpmap_NormalScale").unwrap_or(1.0),
    });
    Some(TextureRef {
        asset_guid: record.guid.clone(),
        slot: String::new(),
        schema: record.schema.clone(),
        paths,
        urn,
        mapping,
        bump,
    })
}

fn property_with_suffix<'a>(
    record: &'a crate::protein::DecodedRecord,
    suffix: &str,
) -> Option<&'a crate::protein::PropertyValue> {
    let qualified_suffix = format!("_{suffix}");
    record
        .properties
        .iter()
        .find(|(id, _)| *id == suffix || id.ends_with(&qualified_suffix))
        .map(|(_, property)| &property.value)
}

fn neutral_property_name(id: &str) -> &str {
    match id {
        "generic_reflectivity_at_0deg" => "reflectivity_at_0deg",
        "generic_refraction_index" | "transparent_refraction_index" => "refraction_index",
        _ => id,
    }
}

fn is_physical_schema(schema: &str) -> bool {
    schema == "PhysMatSchema" || schema.starts_with("Structural") || schema.starts_with("Thermal")
}

fn integer_property(record: &crate::protein::DecodedRecord, suffix: &str) -> Option<u32> {
    match property_with_suffix(record, suffix)? {
        crate::protein::PropertyValue::Integer(value) => Some(*value),
        _ => None,
    }
}

fn float_property(record: &crate::protein::DecodedRecord, suffix: &str) -> Option<f64> {
    match property_with_suffix(record, suffix)? {
        crate::protein::PropertyValue::Float(value) => Some(*value),
        _ => None,
    }
}

fn boolean_property(record: &crate::protein::DecodedRecord, suffix: &str) -> Option<bool> {
    match property_with_suffix(record, suffix)? {
        crate::protein::PropertyValue::Boolean(value) => Some(*value),
        _ => None,
    }
}

fn distance_property(record: &crate::protein::DecodedRecord, suffix: &str) -> Option<f64> {
    let crate::protein::PropertyValue::Distance { unit, value } =
        property_with_suffix(record, suffix)?
    else {
        return None;
    };
    match *unit {
        0x2016 => Some(*value * 25.4),
        0x200e => Some(*value * 10.0),
        _ => None,
    }
}

pub(crate) fn decode_design_assignments(
    scan: &ContainerScan,
) -> Result<Vec<DesignMaterialAssignment>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let body_map = decode_body_map(bytes);
        let strings = lp_utf16_strings(bytes);
        for (index, (_, value)) in strings.iter().enumerate() {
            if !value.starts_with("PrismMaterial") || value.contains("_physmat_aspects") {
                continue;
            }
            let entity_field = strings[..index]
                .iter()
                .rev()
                .take(10)
                .find(|(_, candidate)| entity_suffix(candidate).is_some());
            let Some((entity_offset, entity_id)) = entity_field else {
                continue;
            };
            let entity_suffix = entity_suffix(entity_id).expect(
                "invariant: entity_id was selected because entity_suffix(entity_id) is Some",
            );
            let Some(ba5e_index) = strings
                .iter()
                .enumerate()
                .skip(index + 1)
                .take(15)
                .find_map(|(i, (_, candidate))| {
                    (candidate == "BA5EE55E-9982-449B-9D66-9F036540E140").then_some(i)
                })
            else {
                continue;
            };
            let Some((visual_guid_offset, visual_guid)) =
                ba5e_index.checked_sub(1).and_then(|i| strings.get(i))
            else {
                continue;
            };
            if visual_guid.len() != 36 {
                continue;
            }
            let visual_preset_field = strings
                .get(ba5e_index + 1)
                .filter(|(_, value)| value.starts_with("Prism-"));
            if let Some((&asm_body_key, &(_, key_offset, suffix_offset))) = body_map
                .iter()
                .find(|(_, (suffix, _, _))| *suffix == entity_suffix)
            {
                out.push(DesignMaterialAssignment {
                    id: format!("f3d:{}:material-assignment#{entity_offset}", entry.name),
                    asm_body_key,
                    asm_body_key_offset: key_offset as u64,
                    entity_suffix,
                    entity_suffix_offset: suffix_offset as u64,
                    entity_id: entity_id.clone(),
                    entity_id_offset: (*entity_offset + 4) as u64,
                    visual_guid: visual_guid.clone(),
                    visual_guid_offset: (*visual_guid_offset + 4) as u64,
                    physical_token: Some(value.clone()),
                    physical_token_offset: Some((strings[index].0 + 4) as u64),
                    visual_preset: visual_preset_field.map(|(_, value)| value.clone()),
                    visual_preset_offset: visual_preset_field
                        .map(|(offset, _)| (*offset + 4) as u64),
                });
            }
        }
    }
    Ok(out)
}

/// One per-body appearance override joined to its ASM body key.
pub(crate) struct BodyAppearanceOverride {
    /// The referenced ASM body key from the Design body map.
    pub asm_body_key: u64,
    /// The body's design-entity suffix.
    pub entity_suffix: u64,
    /// First 36 characters of the bound visual GUID.
    pub visual_guid: String,
}

/// Decode per-body appearance overrides from browser body records in every
/// Design `BulkStream` and join them to ASM body keys through the BREP
/// body-map record
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
fn decode_body_appearance_overrides(
    scan: &ContainerScan,
) -> Result<Vec<BodyAppearanceOverride>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let body_map = decode_body_map(bytes);
        for (entity_suffix, visual_guid) in browser_body_appearances(bytes) {
            if let Some((&asm_body_key, _)) = body_map
                .iter()
                .find(|(_, (suffix, _, _))| *suffix == entity_suffix)
            {
                out.push(BodyAppearanceOverride {
                    asm_body_key,
                    entity_suffix,
                    visual_guid,
                });
            }
        }
    }
    Ok(out)
}

/// One per-face appearance assignment from a Design `BulkStream`.
///
/// The face GUID joins the BREP face that carries the same GUID in its
/// `NEUTRON_Material_attrib_def` attribute
/// ([spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials)).
pub struct FaceAppearanceAssignment {
    /// The face GUID shared with the BREP face attribute.
    pub face_guid: String,
    /// First 36 characters of the bound visual GUID.
    pub visual_guid: String,
}

/// Decode per-face appearance assignments from every Design `BulkStream`.
///
/// A face assignment ends with the `BA5EE55E-…` marker GUID; the two
/// length-prefixed UTF-16 strings before the marker are the 36-character
/// face GUID and the bound visual GUID
/// ([spec §8.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#82-materials)).
fn decode_face_appearance_assignments(
    scan: &ContainerScan,
) -> Result<Vec<FaceAppearanceAssignment>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        out.extend(face_appearance_assignments(bytes));
    }
    Ok(out)
}

/// Scan one Design `BulkStream` for face appearance assignments; see
/// [`decode_face_appearance_assignments`].
pub(crate) fn face_appearance_assignments(bytes: &[u8]) -> Vec<FaceAppearanceAssignment> {
    const MARKER: &str = "BA5EE55E-9982-449B-9D66-9F036540E140";
    let strings = lp_utf16_strings(bytes);
    let mut out = Vec::new();
    for (index, (_, value)) in strings.iter().enumerate() {
        if value != MARKER || index < 2 {
            continue;
        }
        let (_, visual) = &strings[index - 1];
        let (_, face_guid) = &strings[index - 2];
        if visual.len() < 36
            || !is_guid_prefix(visual)
            || face_guid.len() != 36
            || !is_guid_prefix(face_guid)
            || face_guid.as_bytes()[0].is_ascii_uppercase()
        {
            continue;
        }
        out.push(FaceAppearanceAssignment {
            face_guid: face_guid.clone(),
            visual_guid: visual[..36].to_string(),
        });
    }
    out
}

/// The marker GUID pair that opens the appearance fields of a browser body
/// record ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
const BODY_RECORD_MARKER_GUIDS: [&str; 2] = [
    "D87FBE62-3B12-4CA8-9014-BAD31ABDB101",
    "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C",
];

/// Scan a Design `BulkStream` for browser body records that bind an
/// appearance and return `(body entity suffix, 36-character visual GUID)`
/// pairs.
///
/// A browser body record carries a `299`-tagged head whose entity is the
/// body's design-entity suffix, the marker GUID pair, the physical-material
/// token, the browser-node GUID with the node's entity (the body suffix plus
/// one), the display name, an f32 opacity, the `01 01` marker, and the bound
/// visual GUID ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
/// The scan requires the head entity and node entity to agree before
/// accepting a record.
pub(crate) fn browser_body_appearances(bytes: &[u8]) -> Vec<(u64, String)> {
    let marker: Vec<u8> = lp_utf16_bytes(BODY_RECORD_MARKER_GUIDS[0])
        .into_iter()
        .chain(lp_utf16_bytes(BODY_RECORD_MARKER_GUIDS[1]))
        .collect();
    let mut out = Vec::new();
    let mut position = 0usize;
    while let Some(at) = find(bytes, &marker, position) {
        position = at + marker.len();
        let Some(entity_suffix) = browser_body_appearance_at(bytes, at, position) else {
            continue;
        };
        out.push(entity_suffix);
    }
    out
}

/// Parse the appearance fields of one browser body record whose marker GUID
/// pair spans `marker_at..fields_at`; see [`browser_body_appearances`].
fn browser_body_appearance_at(
    bytes: &[u8],
    marker_at: usize,
    fields_at: usize,
) -> Option<(u64, String)> {
    // Physical-material token, then its entity reference.
    let (token, after) = lp_utf16_bounded(bytes, skip_zeros(bytes, fields_at), 1..=256)?;
    if !token.starts_with("PrismMaterial") || bytes.get(after)? != &0x01 {
        return None;
    }
    // Browser-node GUID, then the node's entity.
    let (node_guid, after) = lp_utf16_bounded(bytes, skip_zeros(bytes, after + 9), 1..=256)?;
    if node_guid.len() != 36 || !is_guid_prefix(&node_guid) || bytes.get(after)? != &0x01 {
        return None;
    }
    let node_entity = u64_at(bytes, after + 1)?;
    // Optional display name, opacity, and the `01 01` marker.
    let name_end = match lp_utf16_bounded(bytes, skip_zeros(bytes, after + 9), 1..=256) {
        Some((_, end)) => end,
        None => after + 9,
    };
    let visual_at = record_tail_visual_offset(bytes, name_end)?;
    let (visual, _) = lp_utf16_bounded(bytes, visual_at, 1..=256)?;
    if visual.len() < 36 || !is_guid_prefix(&visual) {
        return None;
    }
    // The record head's `299` class tag names the body's design entity; it
    // precedes the marker pair and equals the node entity minus one.
    let head_entity = preceding_class_299_entity(bytes, marker_at)?;
    if head_entity + 1 != node_entity {
        return None;
    }
    Some((head_entity, visual[..36].to_string()))
}

/// Skip the zeros and f32 opacity between a body record's display name and
/// its `01 01` marker and return the visual GUID's length-prefix offset.
fn record_tail_visual_offset(bytes: &[u8], name_end: usize) -> Option<usize> {
    const OPACITY_ONE: [u8; 4] = [0x00, 0x00, 0x80, 0x3f];
    for delta in 0..40usize {
        let at = name_end + delta;
        if bytes.get(at..at + 2)? != [0x01, 0x01] {
            continue;
        }
        let gap = &bytes[name_end..at];
        let zeros_only = gap.iter().all(|byte| *byte == 0);
        let opacity_tail = gap.len() >= 4
            && gap[gap.len() - 4..] == OPACITY_ONE
            && gap[..gap.len() - 4].iter().all(|byte| *byte == 0);
        if !(zeros_only || opacity_tail) {
            return None;
        }
        return Some(skip_zeros_capped(bytes, at + 2, 12));
    }
    None
}

/// Find the `u32 3 + "299"` class tag nearest before `at` and read its
/// entity value.
fn preceding_class_299_entity(bytes: &[u8], at: usize) -> Option<u64> {
    const CLASS_299: [u8; 7] = [3, 0, 0, 0, b'2', b'9', b'9'];
    let window_start = at.saturating_sub(65536);
    let window = bytes.get(window_start..at)?;
    let tag_at = window
        .windows(CLASS_299.len())
        .rposition(|candidate| candidate == CLASS_299)?;
    u64_at(bytes, window_start + tag_at + CLASS_299.len())
}

/// Encode a string as its length-prefixed UTF-16 byte form.
/// Advance past at most `cap` zero bytes starting at `position`.
fn skip_zeros_capped(bytes: &[u8], position: usize, cap: usize) -> usize {
    let mut at = position;
    while at < bytes.len() && at - position < cap && bytes[at] == 0 {
        at += 1;
    }
    at
}

/// Advance past at most eight zero bytes starting at `position`.
fn skip_zeros(bytes: &[u8], position: usize) -> usize {
    skip_zeros_capped(bytes, position, 8)
}

fn bind_bodies<S: std::hash::BuildHasher>(
    appearances: &[Appearance],
    assignments: &[DesignMaterialAssignment],
    act_channels: &std::collections::HashMap<u64, BTreeMap<String, String>>,
    object_types: &std::collections::HashMap<u64, String>,
    body_keys: &std::collections::HashMap<BodyId, u64, S>,
) -> Vec<AppearanceBinding> {
    assignments
        .iter()
        .filter_map(|assignment| {
            let body = body_keys.iter().find_map(|(body, key)| {
                (*key == assignment.asm_body_key).then_some(body.clone())
            })?;
            let appearance = appearances.iter().find(|appearance| {
                appearance
                    .visual_guid
                    .as_deref()
                    .is_some_and(|guid| visual_guid_matches(guid, &assignment.visual_guid))
                    || assignment.visual_preset.as_deref() == appearance.name.as_deref()
            })?;
            Some(AppearanceBinding {
                id: format!(
                    "f3d:appearance:binding#{}:{}",
                    assignment.entity_id, assignment.visual_guid
                ),
                target: AppearanceTarget::Body(body),
                appearance: appearance.id.clone(),
                source_entity_id: Some(assignment.entity_id.clone()),
                object_type: object_types.get(&assignment.entity_suffix).cloned(),
                channels: act_channels
                    .get(&assignment.entity_suffix)
                    .cloned()
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn decode_design_object_types(
    scan: &ContainerScan,
) -> Result<std::collections::HashMap<u64, String>, CodecError> {
    let mut out = std::collections::HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 8 <= bytes.len() {
            let Some((object_type, after_type)) =
                lp_ascii_filtered(bytes, position, 1..=64, |byte| (0x20..0x7f).contains(byte))
            else {
                position += 1;
                continue;
            };
            if !object_type.chars().all(char::is_alphabetic) {
                position += 1;
                continue;
            }
            let Some(count_bytes) = bytes.get(after_type..after_type + 4) else {
                break;
            };
            let count = u32::from_le_bytes(count_bytes.try_into().expect(
                "invariant: count_bytes is a 4-byte slice from bytes.get(range) of length 4",
            )) as usize;
            if count > 200 || after_type + 4 + count * 8 > bytes.len() {
                position += 1;
                continue;
            }
            for id_bytes in bytes[after_type + 4..after_type + 4 + count * 8].chunks_exact(8) {
                out.insert(
                    u64::from_le_bytes(
                        id_bytes
                            .try_into()
                            .expect("invariant: chunks_exact(8) yields 8-byte slices"),
                    ),
                    object_type.clone(),
                );
            }
            position = after_type + 4 + count * 8;
        }
    }
    Ok(out)
}

fn decode_act_channels(
    scan: &ContainerScan,
) -> Result<std::collections::HashMap<u64, BTreeMap<String, String>>, CodecError> {
    let mut out = std::collections::HashMap::new();
    for entry in scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM && entry.name.contains("FusionACTSegmentType")
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut position = 0usize;
        while position + 4 <= bytes.len() {
            let Some((tag, after_tag)) =
                lp_ascii_filtered(bytes, position, 1..=64, |byte| (0x20..0x7f).contains(byte))
            else {
                position += 1;
                continue;
            };
            if tag.len() != 3 || !tag.bytes().all(|byte| byte.is_ascii_digit()) {
                position += 1;
                continue;
            }
            let Some(header) = bytes.get(after_tag..after_tag + 18) else {
                break;
            };
            if header.get(4..14) != Some(&[0u8; 10]) {
                position += 1;
                continue;
            }
            let count = u32::from_le_bytes(
                header[14..18]
                    .try_into()
                    .expect("invariant: header is an 18-byte slice, so header[14..18] is 4 bytes"),
            ) as usize;
            if !(1..=8).contains(&count) {
                position += 1;
                continue;
            }
            let mut cursor = after_tag + 18;
            let mut channels = BTreeMap::new();
            let mut valid = true;
            for _ in 0..count {
                let Some((name, after_name)) =
                    lp_ascii_filtered(bytes, cursor, 1..=64, |byte| (0x20..0x7f).contains(byte))
                else {
                    valid = false;
                    break;
                };
                let Some((guid, after_guid)) = lp_utf16_bounded(bytes, after_name, 1..=64) else {
                    valid = false;
                    break;
                };
                if guid.len() != 36 {
                    valid = false;
                    break;
                }
                channels.insert(name, guid);
                cursor = after_guid;
            }
            if valid {
                if let Some((entity, end)) = lp_utf16_bounded(bytes, cursor, 1..=64) {
                    if let Some(suffix) = entity_suffix(&entity) {
                        out.insert(suffix, channels);
                    }
                    position = end;
                    continue;
                }
            }
            position += 1;
        }
    }
    Ok(out)
}

fn entity_suffix(value: &str) -> Option<u64> {
    let (_, suffix) = value.split_once('_')?;
    suffix.parse().ok()
}

fn lp_utf16_strings(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + 4 <= bytes.len() {
        if let Some((value, record_len)) = lp_utf16_string_at(bytes, offset) {
            out.push((offset, value));
            offset += record_len;
        } else {
            offset += 1;
        }
    }
    out
}

/// Decode one LP-UTF16 string at `offset`, validating unit by unit so a
/// non-string byte window bails out before allocating.
fn lp_utf16_string_at(bytes: &[u8], offset: usize) -> Option<(String, usize)> {
    let count = usize::try_from(u32_at(bytes, offset)?).ok()?;
    if !(2..=256).contains(&count) {
        return None;
    }
    let byte_len = count * 2;
    let payload = bytes.get(offset + 4..offset + 4 + byte_len)?;
    let mut value = String::new();
    for unit in char::decode_utf16(
        payload
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]])),
    ) {
        let ch = unit.ok()?;
        if ch.is_control() {
            return None;
        }
        value.push(ch);
    }
    Some((value, 4 + byte_len))
}

fn decode_body_map(bytes: &[u8]) -> std::collections::HashMap<u64, (u64, usize, usize)> {
    crate::design::decode::body::body_bindings(bytes)
        .into_iter()
        .map(|binding| {
            (
                binding.asm_key,
                (
                    binding.entity_suffix,
                    binding.asm_key_offset,
                    binding.entity_suffix_offset,
                ),
            )
        })
        .collect()
}

fn instance_properties(protein: &[u8]) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(protein)).ok()?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).ok()?;
        if file.name().ends_with("AssetData/InstanceProperties.bin") {
            let size = file.size();
            let name = file.name().to_owned();
            let bytes = crate::container::read_entry_bounded(&mut file, size, &name).ok()?;
            return Some(bytes);
        }
    }
    None
}

fn definition_catalog(
    protein: &[u8],
) -> std::collections::HashMap<String, (String, Option<String>)> {
    let Some(bytes) = nested_entry(protein, "AssetData/DefinitionIteratorProperties.bin") else {
        return std::collections::HashMap::new();
    };
    let marker = b"\x80\x00\x01\x00";
    let starts: Vec<usize> = bytes
        .windows(marker.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == marker).then_some(offset))
        .collect();
    let mut out = std::collections::HashMap::new();
    for (index, start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(bytes.len());
        let mut strings = Vec::new();
        let mut position = *start + marker.len();
        while position + 4 <= end && strings.len() < 8 {
            let length = u32::from_le_bytes(
                bytes[position..position + 4]
                    .try_into()
                    .expect("invariant: bytes[position..position+4] is a 4-byte slice"),
            ) as usize;
            if (1..=200).contains(&length) && position + 4 + length <= end {
                let raw = &bytes[position + 4..position + 4 + length];
                if raw.iter().all(|byte| (0x20..=0x7e).contains(byte)) {
                    strings.push(String::from_utf8_lossy(raw).into_owned());
                    position += 4 + length;
                    continue;
                }
            }
            position += 1;
        }
        if strings
            .first()
            .is_some_and(|schema| schema.ends_with("Schema"))
        {
            if let Some(asset_id) = strings.get(1) {
                out.insert(
                    asset_id.clone(),
                    (strings[0].clone(), strings.get(3).cloned()),
                );
            }
        }
    }
    out
}

fn nested_entry(protein: &[u8], suffix: &str) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(protein)).ok()?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).ok()?;
        if file.name().ends_with(suffix) {
            let size = file.size();
            let name = file.name().to_owned();
            let bytes = crate::container::read_entry_bounded(&mut file, size, &name).ok()?;
            return Some(bytes);
        }
    }
    None
}

fn dechunk(bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < 16 + PAGE_SIZE
        || u32::from_le_bytes(bytes.get(0..4)?.try_into().ok()?) as usize != PAGE_SIZE
        || !(bytes.len() - 16).is_multiple_of(PAGE_SIZE)
    {
        return None;
    }
    let mut out = Vec::new();
    for page in bytes[16..].chunks_exact(PAGE_SIZE) {
        if page.get(4..8) == Some(RECORD_MARKER) {
            out.extend_from_slice(&page[4..]);
        } else if page.get(4..8) == Some(b"\x80\x00\x00\x00") {
            out.extend_from_slice(&page[8..]);
        } else if page.get(0..4) == Some(b"\xff\xff\xff\xff") {
            let used = u16::from_le_bytes(page.get(4..6)?.try_into().ok()?) as usize;
            out.extend_from_slice(page.get(8..8 + used)?);
        } else {
            return None;
        }
    }
    Some(out)
}

/// Decode the fixed source-less layouts emitted by [`encode_protein`]. Native
/// Protein assets package schemas and use the schema-driven path instead.
fn decode_fixed_logical_records(bytes: &[u8]) -> Vec<Appearance> {
    let starts = bytes
        .windows(RECORD_MARKER.len())
        .enumerate()
        .filter_map(|(offset, marker)| (marker == RECORD_MARKER).then_some(offset))
        .collect::<Vec<_>>();
    starts
        .iter()
        .enumerate()
        .filter_map(|(ordinal, start)| {
            let end = starts.get(ordinal + 1).copied().unwrap_or(bytes.len());
            decode_fixed_record(&bytes[*start..end])
        })
        .collect()
}

fn decode_fixed_record(record: &[u8]) -> Option<Appearance> {
    let mut position = RECORD_MARKER.len();
    let schema = take_lp_utf8(record, &mut position)?;
    let guid = take_lp_utf8(record, &mut position)?;
    let base = take_lp_utf8(record, &mut position)?;
    take_lp_utf8(record, &mut position)?;
    let color = match schema.as_str() {
        "GenericSchema" => fixed_rgba(
            record,
            position + 112 + generic_connection_delta(record, position),
        ),
        "PrismOpaqueSchema" | "PrismMetalSchema" => fixed_rgba(record, position + 8),
        "PrismTransparentSchema" => fixed_rgba(record, position + 121),
        "PhysMatSchema"
        | "StructuralMetalSchema"
        | "StructuralPlasticSchema"
        | "ThermalSolidSchema" => None,
        _ => return None,
    };
    let mut properties = BTreeMap::new();
    if schema == "GenericSchema" {
        let delta = generic_connection_delta(record, position);
        fixed_tagged_scalar(
            &mut properties,
            "reflectivity_at_0deg",
            record,
            position + 171 + delta,
        );
        fixed_tagged_scalar(
            &mut properties,
            "refraction_index",
            record,
            position + 197 + delta,
        );
    } else if schema == "PrismOpaqueSchema" {
        if let Some(marker) = find(record, b"\x0e\x20\x00\x00", position) {
            fixed_scalar(&mut properties, "surface_roughness", record, marker + 4);
        }
    } else if schema == "PrismTransparentSchema" {
        fixed_scalar(&mut properties, "refraction_index", record, position + 169);
    }
    Some(Appearance {
        id: AppearanceId(format!("f3d:design:appearance#{guid}")),
        name: Some(base),
        asset_guid: Some(guid.clone()),
        visual_guid: (!matches!(
            schema.as_str(),
            "PhysMatSchema"
                | "StructuralMetalSchema"
                | "StructuralPlasticSchema"
                | "ThermalSolidSchema"
        ))
        .then_some(guid),
        physical_token: None,
        schema: Some(schema),
        category: None,
        base_color: color,
        properties,
        textures: Vec::new(),
    })
}

fn fixed_scalar(out: &mut BTreeMap<String, f64>, name: &str, bytes: &[u8], offset: usize) {
    let Some(raw) = bytes
        .get(offset..offset + 8)
        .and_then(|raw| raw.try_into().ok())
    else {
        return;
    };
    let value = f64::from_le_bytes(raw);
    if value.is_finite() {
        out.insert(name.to_owned(), value);
    }
}

fn fixed_tagged_scalar(out: &mut BTreeMap<String, f64>, name: &str, bytes: &[u8], offset: usize) {
    if bytes.get(offset..offset + 4) == Some(b"\x0c\x00\x00\x00") {
        fixed_scalar(out, name, bytes, offset + 4);
    }
}

fn fixed_rgba(bytes: &[u8], offset: usize) -> Option<Color> {
    let mut values = [0.0; 4];
    for (ordinal, value) in values.iter_mut().enumerate() {
        let at = offset + ordinal * 8;
        *value = f64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?);
    }
    decoded_color(values)
}

fn generic_connection_delta(record: &[u8], value_block: usize) -> usize {
    let slot = value_block + 102;
    match record.get(slot) {
        Some(0) => 0,
        Some(1) if slot + 6 <= record.len() => {
            let count = u32::from_le_bytes(
                record[slot + 2..slot + 6]
                    .try_into()
                    .expect("invariant: record[slot+2..slot+6] is a 4-byte slice"),
            ) as usize;
            let mut position = slot + 6;
            for _ in 0..count.min(8) {
                let Some(length_bytes) = record.get(position..position + 4) else {
                    return 0;
                };
                let length = u32::from_le_bytes(length_bytes.try_into().expect(
                    "invariant: length_bytes is a 4-byte slice from bytes.get(range) of length 4",
                )) as usize;
                position += 4;
                if record.get(position..position + length).is_none() {
                    return 0;
                }
                position += length;
            }
            position.saturating_sub(slot + 1)
        }
        _ => 0,
    }
}

fn find(bytes: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    bytes
        .get(start..)?
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|offset| start + offset)
}

#[cfg(test)]
mod tests {
    #[test]
    fn decoded_color_requires_finite_normalized_channels() {
        assert!(super::decoded_color([0.0, 0.25, 0.5, 1.0]).is_some());
        for invalid in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert!(super::decoded_color([invalid, 0.25, 0.5, 1.0]).is_none());
        }
    }
}
