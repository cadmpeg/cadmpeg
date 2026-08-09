// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion `.protein` appearance assets and bind them to B-rep bodies.
//!
//! Material and appearance semantics are defined in [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
//! [`decode`] reads appearance records without resolving body bindings.
//! [`decode_with_body_bindings`] joins Protein assets, Design assignments, ACT
//! channels, and blob-qualified Design body-map bindings through the
//! design-entity join backbone in
//! [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use crate::records::{DesignBodyBinding, DesignMaterialAssignment};
use cadmpeg_core::le::u32_at;
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{
    Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef,
};
use cadmpeg_ir::ids::{AppearanceId, BodyId};
use cadmpeg_ir::topology::Color;

use crate::bytes::{is_guid_prefix, lp_ascii_filtered, lp_utf16_bounded, take_lp_utf8};
use crate::container::{role, ContainerScan};
use crate::design::presentation::{
    visual_token, APPEARANCE_LIBRARY_ID, GUID_LEN,
    MODERN_APPEARANCE_LIBRARY_IDS as APPEARANCE_LIBRARY_ID_PAIR,
};

const PAGE_SIZE: usize = 0x88;
const RECORD_MARKER: &[u8] = b"\x80\x00\x01\x00";
/// The `AssetLibID` [`encode_protein`] writes for an appearance that names no
/// library. A stored library identifier is a library GUID or a library path;
/// the null GUID names neither.
const NO_ASSET_LIB_ID: &str = "00000000-0000-0000-0000-000000000000";

/// The library identifier an `InstanceProperties` record stores, when it names
/// a library.
fn library_id(asset_lib_id: &str) -> Option<String> {
    (!asset_lib_id.is_empty() && asset_lib_id != NO_ASSET_LIB_ID).then(|| asset_lib_id.to_owned())
}

/// Whether two complete serialized visual tokens identify one appearance
/// record.
pub(crate) fn visual_tokens_match(left: &str, right: &str) -> bool {
    visual_token(left)
        .zip(visual_token(right))
        .is_some_and(|(left, right)| left.matches(right))
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
    for value in [
        schema,
        guid,
        name,
        appearance.library_id.as_deref().unwrap_or(NO_ASSET_LIB_ID),
    ] {
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
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
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
        let options = crate::zip_write::file_options(entry.compression());
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
            patch_instance_colors(protein, &mut bytes, edits, &mut patched)?;
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
    protein: &[u8],
    bytes: &mut [u8],
    edits: &BTreeMap<String, ProteinAppearanceEdit>,
    patched: &mut std::collections::BTreeSet<String>,
) -> Result<(), CodecError> {
    let frames = crate::protein::record_frames(bytes).ok_or_else(|| {
        CodecError::Malformed("cannot frame Protein InstanceProperties pages".into())
    })?;
    let schema_driven = crate::protein::has_schemas(protein);
    let decoded = if schema_driven {
        crate::protein::decode(protein, bytes)?
    } else {
        Vec::new()
    };
    for frame in frames {
        let record = frame.bytes.as_slice();
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
        let decoded_record = if schema_driven {
            Some(
                decoded
                    .iter()
                    .find(|decoded| {
                        decoded.logical_offset == frame.logical_offset
                            && decoded.schema == schema
                            && decoded.guid == guid
                    })
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "Protein appearance {guid} has no decoded schema record"
                        ))
                    })?,
            )
        } else {
            None
        };
        if let Some(color) = edit.color {
            let relative = if let Some(decoded_record) = decoded_record {
                let property_id =
                    appearance_base_color_property_id(decoded_record).ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "Protein appearance {guid} has no schema-selected color carrier"
                        ))
                    })?;
                let property = decoded_record
                    .properties
                    .get(property_id)
                    .filter(|property| {
                        matches!(&property.value, crate::protein::PropertyValue::Color(_))
                    })
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "Protein appearance {guid} has no {property_id} color carrier"
                        ))
                    })?;
                property.value_offset
            } else {
                match schema.as_str() {
                    "GenericSchema" => {
                        position
                            + 112
                            + generic_connection_delta(record, position).ok_or_else(|| {
                                CodecError::Malformed(
                                    "Protein GenericSchema connection list is malformed".into(),
                                )
                            })?
                    }
                    "PrismOpaqueSchema" | "PrismMetalSchema" => position + 8,
                    "PrismTransparentSchema" => position + 121,
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "Protein schema {schema} has no writable color carrier"
                        )))
                    }
                }
            };
            for (ordinal, value) in [color.r, color.g, color.b, color.a].into_iter().enumerate() {
                patch_logical_f64(
                    bytes,
                    frame.logical_offset + relative + ordinal * 8,
                    f64::from(value),
                )?;
            }
        }
        for (name, value) in &edit.properties {
            let relative = if let Some(decoded_record) = decoded_record {
                let property_id = match (schema.as_str(), name.as_str()) {
                    ("GenericSchema", "reflectivity_at_0deg") => "generic_reflectivity_at_0deg",
                    ("GenericSchema", "refraction_index") => "generic_refraction_index",
                    ("PrismOpaqueSchema", "surface_roughness") => "surface_roughness",
                    ("PrismTransparentSchema", "refraction_index") => {
                        "transparent_refraction_index"
                    }
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "Protein schema {schema} property {name} has no writable carrier"
                        )))
                    }
                };
                decoded_record
                    .properties
                    .get(property_id)
                    .filter(|property| {
                        matches!(&property.value, crate::protein::PropertyValue::Float(_))
                    })
                    .map(|property| property.value_offset)
                    .ok_or_else(|| {
                        CodecError::Malformed(format!(
                            "Protein appearance {guid} has no {property_id} scalar carrier"
                        ))
                    })?
            } else {
                match (schema.as_str(), name.as_str()) {
                    ("GenericSchema", "reflectivity_at_0deg") => {
                        position
                            + 175
                            + generic_connection_delta(record, position).ok_or_else(|| {
                                CodecError::Malformed(
                                    "Protein GenericSchema connection list is malformed".into(),
                                )
                            })?
                    }
                    ("GenericSchema", "refraction_index") => {
                        position
                            + 201
                            + generic_connection_delta(record, position).ok_or_else(|| {
                                CodecError::Malformed(
                                    "Protein GenericSchema connection list is malformed".into(),
                                )
                            })?
                    }
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
                }
            };
            patch_logical_f64(bytes, frame.logical_offset + relative, *value)?;
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
/// [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
#[derive(Default)]
pub struct DecodedMaterials {
    /// Merged appearance records, deduplicated by [`AppearanceId`].
    pub appearances: Vec<Appearance>,
    /// Body-to-appearance bindings resolved through ACT and Design body-map joins.
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

/// Decode `.protein` assets and Design and ACT assignments without resolved
/// Design body-map bindings.
///
/// The [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)
/// Design body-map join is skipped. Use [`decode_with_body_bindings`] when the
/// resolved map pairs are available.
pub fn decode(scan: &ContainerScan) -> Result<DecodedMaterials, CodecError> {
    decode_with_body_bindings(scan, &[])
}

/// Decode appearance assets and resolve body bindings through the ordered,
/// blob-qualified Design body-map pairs, closing the design-entity join
/// backbone in [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
pub fn decode_with_body_bindings(
    scan: &ContainerScan,
    body_bindings: &[DesignBodyBinding],
) -> Result<DecodedMaterials, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_asset_entry(entry, role::PROTEIN))
    {
        let payload = scan.entry_bytes(&entry.name)?;
        let Some(instance) = instance_properties(payload) else {
            continue;
        };
        let Some(record_frames) = crate::protein::record_frames(&instance) else {
            continue;
        };
        let catalog = definition_catalog(payload);
        let mut appearances = if crate::protein::has_schemas(payload) {
            let records = crate::protein::decode(payload, &instance)?;
            let mut decoded = appearances_from_schema_records(&records);
            let decoded_ids = decoded
                .iter()
                .map(|appearance| appearance.id.clone())
                .collect::<std::collections::HashSet<_>>();
            decoded.extend(
                decode_fixed_logical_records(&record_frames)
                    .into_iter()
                    .filter(|appearance| !decoded_ids.contains(&appearance.id)),
            );
            decoded
        } else {
            decode_fixed_logical_records(&record_frames)
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
    if let Some(pair) = out
        .windows(2)
        .find(|pair| pair[0].id == pair[1].id && pair[0] != pair[1])
    {
        return Err(CodecError::Malformed(format!(
            "F3D appearance asset {} has conflicting payloads",
            pair[0].id
        )));
    }
    out.dedup_by(|a, b| a.id == b.id);
    let assignments = decode_design_assignments(scan)?;
    let act_channels = decode_act_channels(scan)?;
    let object_types = decode_design_object_types(scan)?;
    for assignment in &assignments {
        if appearance_for_assignment(&out, assignment)?.is_none() {
            out.push(Appearance {
                id: AppearanceId(format!("f3d:design:appearance#{}", assignment.visual_guid)),
                name: assignment.visual_preset.clone(),
                asset_guid: Some(assignment.visual_guid.clone()),
                library_id: None,
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
                .is_some_and(|guid| visual_tokens_match(guid, &assignment.visual_guid))
        }) {
            appearance.physical_token = assignment.physical_token.clone();
        }
    }
    let mut bindings = bind_bodies(
        &out,
        &assignments,
        &act_channels,
        &object_types,
        body_bindings,
    )?;
    let body_overrides = decode_body_appearance_overrides(scan, body_bindings)?;
    for over in &body_overrides {
        if bindings
            .iter()
            .any(|binding| binding.target == AppearanceTarget::Body(over.body.clone()))
        {
            continue;
        }
        let Some(appearance) = appearance_for_visual_token(&out, &over.visual_guid, None)? else {
            continue;
        };
        bindings.push(AppearanceBinding {
            id: format!(
                "f3d:appearance:body#{}:{}",
                over.entity_suffix, over.visual_guid
            ),
            target: AppearanceTarget::Body(over.body.clone()),
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
            let base_color = appearance_base_color(record);
            Appearance {
                id: AppearanceId(format!("f3d:design:appearance#{}", record.guid)),
                name: Some(record.base.clone()),
                asset_guid: Some(record.guid.clone()),
                library_id: library_id(&record.asset_lib_id),
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

/// Resolve the one schema member that supplies an appearance's neutral base
/// colour. An enabled common tint replaces the shader family's primary colour;
/// a disabled or absent tint does not participate in selection.
fn appearance_base_color(record: &crate::protein::DecodedRecord) -> Option<Color> {
    color_property(record, appearance_base_color_property_id(record)?)
}

/// Select the serialized color carrier that represents the neutral base color.
fn appearance_base_color_property_id(
    record: &crate::protein::DecodedRecord,
) -> Option<&'static str> {
    if matches!(
        record
            .properties
            .get("common_Tint_toggle")
            .map(|property| &property.value),
        Some(crate::protein::PropertyValue::Boolean(true))
    ) {
        return Some("common_Tint_color");
    }

    let id = match record.schema.as_str() {
        "GenericSchema" => "generic_diffuse",
        "MetalSchema" => "metal_color",
        "MetallicPaintSchema" => "metallicpaint_base_color",
        "PlasticVinylSchema" => "plasticvinyl_color",
        "PrismLayeredSchema" => "layered_diffuse",
        "PrismMetalSchema" => "metal_f0",
        "PrismOpaqueSchema" => "opaque_albedo",
        "PrismTransparentSchema" => "transparent_color",
        // `PrismCommonSchema` supplies the common fallback used by derived
        // families that do not define one primary constant-colour member.
        _ if record.properties.contains_key("surface_albedo") => "surface_albedo",
        _ => return None,
    };
    Some(id)
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
        0x200e => Some(*value),
        0x200d => Some(*value * 10.0),
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
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let body_map = crate::design::decode::body::body_bindings(bytes, &metadata)?;
        for presentation in
            crate::design::decode::presentation::body_presentations(bytes, &metadata)?
        {
            let Some(material) = presentation.material else {
                continue;
            };
            let crate::design::decode::presentation::BodyPresentationOwner::Named {
                entity_id,
                entity_id_offset,
            } = presentation.owner
            else {
                continue;
            };
            let Some(body_binding) =
                unique_body_map_pair(&body_map, presentation.entity_suffix, "material assignment")?
            else {
                continue;
            };
            out.push(DesignMaterialAssignment {
                id: crate::ids::native_scoped_id(
                    &entry.name,
                    "material-assignment",
                    presentation.byte_offset as usize,
                ),
                asm_body_key: body_binding.asm_key,
                asm_body_key_offset: body_binding.asm_key_offset as u64,
                entity_suffix: presentation.entity_suffix,
                entity_suffix_offset: body_binding.entity_suffix_offset as u64,
                entity_id,
                entity_id_offset,
                visual_guid: material.visual_guid,
                visual_guid_offset: material.visual_guid_offset,
                physical_token: Some(material.physical_token),
                physical_token_offset: Some(material.physical_token_offset),
                visual_preset: material.visual_preset,
                visual_preset_offset: material.visual_preset_offset,
            });
        }
    }
    Ok(out)
}

/// One per-body appearance override joined through its exact Design body-map pair.
pub(crate) struct BodyAppearanceOverride {
    /// Solved body selected by the exact blob-qualified body-map pair.
    pub body: BodyId,
    /// The body's design-entity suffix.
    pub entity_suffix: u64,
    /// Complete serialized visual token bound by the body record.
    pub visual_guid: String,
}

/// Decode per-body appearance overrides from browser body records in every
/// Design `BulkStream` and join them through the exact BREP body-map pair
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
fn decode_body_appearance_overrides(
    scan: &ContainerScan,
    body_bindings: &[DesignBodyBinding],
) -> Result<Vec<BodyAppearanceOverride>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let body_map = crate::design::decode::body::body_bindings(bytes, &metadata)?;
        let mut appearances = browser_body_appearances(bytes);
        appearances.extend(
            crate::design::decode::presentation::body_presentations(bytes, &metadata)?
                .into_iter()
                .filter_map(|presentation| {
                    if presentation.owner
                        != crate::design::decode::presentation::BodyPresentationOwner::Bare
                        || presentation.browser_node.is_none()
                    {
                        return None;
                    }
                    Some((
                        presentation.entity_suffix,
                        presentation.material?.visual_guid,
                    ))
                }),
        );
        for (entity_suffix, visual_guid) in appearances {
            let Some(map_pair) =
                unique_body_map_pair(&body_map, entity_suffix, "browser body appearance")?
            else {
                continue;
            };
            let Some(body) = resolved_body_for_map_pair(
                body_bindings,
                &crate::ids::native_design_body_binding_id(&entry.name, map_pair.asm_key_offset),
                map_pair.asm_key,
                map_pair.asm_key_offset as u64,
                map_pair.entity_suffix,
                map_pair.entity_suffix_offset as u64,
            )?
            else {
                continue;
            };
            out.push(BodyAppearanceOverride {
                body,
                entity_suffix,
                visual_guid,
            });
        }
    }
    out.sort_by(|left, right| {
        left.body
            .cmp(&right.body)
            .then_with(|| left.entity_suffix.cmp(&right.entity_suffix))
            .then_with(|| left.visual_guid.cmp(&right.visual_guid))
    });
    out.dedup_by(|left, right| {
        left.body == right.body
            && left.entity_suffix == right.entity_suffix
            && visual_tokens_match(&left.visual_guid, &right.visual_guid)
    });
    Ok(out)
}

/// One per-face appearance assignment from a Design `BulkStream`.
///
/// The face GUID joins the BREP face that carries the same GUID in its
/// `NEUTRON_Material_attrib_def` attribute
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
#[derive(Debug, Clone, PartialEq)]
pub struct FaceAppearanceAssignment {
    /// The face GUID shared with the BREP face attribute.
    pub face_guid: String,
    /// Complete serialized visual token bound by the face record.
    pub visual_guid: String,
    /// Face-local neutral color carried by a legacy assignment entry.
    pub color: Option<Color>,
}

/// Decode per-face appearance assignments from every Design `BulkStream`.
///
/// A legacy face assignment ends with the `BA5EE55E-…` marker GUID. A current
/// assignment ends with the paired-library tail decoded by
/// [`modern_face_appearance_assignments`]. Both forms stay inside one exact
/// primary-index frame
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
fn decode_face_appearance_assignments(
    scan: &ContainerScan,
) -> Result<Vec<FaceAppearanceAssignment>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::BULKSTREAM))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        for frame in crate::metastream::primary_record_frames(&metadata, bytes.len())? {
            out.extend(face_appearance_assignments_in_frame(
                &bytes[frame.start..frame.end],
            ));
        }
    }
    Ok(out)
}

/// Decode a synthetic test slice as one Design primary-index frame.
#[cfg(test)]
pub(crate) fn face_appearance_assignments(bytes: &[u8]) -> Vec<FaceAppearanceAssignment> {
    face_appearance_assignments_in_frame(bytes)
}

/// Decode face assignments from one exact Design primary-index frame.
fn face_appearance_assignments_in_frame(bytes: &[u8]) -> Vec<FaceAppearanceAssignment> {
    let strings = lp_utf16_strings(bytes);
    let mut out = legacy_face_appearance_assignments(bytes, &strings);
    out.extend(modern_face_appearance_assignments(bytes, &strings));
    out
}

/// Decode the variable-width legacy face-assignment envelope.
///
/// Every accepted member is adjacent to the next one. This excludes other
/// body-presentation records that share the appearance-library marker.
fn legacy_face_appearance_assignments(
    bytes: &[u8],
    strings: &[(usize, String)],
) -> Vec<FaceAppearanceAssignment> {
    const LP_GUID_BYTES: usize = 4 + GUID_LEN * 2;
    const COLOR_BYTES: usize = 4 * size_of::<f32>();
    const CARRIER_BYTES: usize = 12;

    let mut out = Vec::new();
    for (index, (marker_at, marker)) in strings.iter().enumerate() {
        if marker != APPEARANCE_LIBRARY_ID {
            continue;
        }
        let Some((visual_at, visual)) = index.checked_sub(1).and_then(|at| strings.get(at)) else {
            continue;
        };
        let Some((_, visual_len)) = lp_utf16_string_at(bytes, *visual_at) else {
            continue;
        };
        if visual_at.checked_add(visual_len) != Some(*marker_at) || visual_token(visual).is_none() {
            continue;
        }

        let Some(face_at) = visual_at.checked_sub(LP_GUID_BYTES + COLOR_BYTES + CARRIER_BYTES)
        else {
            continue;
        };
        let Some((face_guid, face_len)) = lp_utf16_string_at(bytes, face_at) else {
            continue;
        };
        if face_len != LP_GUID_BYTES || !is_lowercase_guid(&face_guid) {
            continue;
        }
        let color_at = face_at + face_len;
        let Some(color) = normalized_legacy_face_color(bytes, color_at) else {
            continue;
        };
        let carrier_at = color_at + COLOR_BYTES;
        let Some(selector_kind) =
            legacy_face_selector_kind(bytes.get(carrier_at..carrier_at + CARRIER_BYTES))
        else {
            continue;
        };
        if carrier_at + CARRIER_BYTES != *visual_at {
            continue;
        }

        let Some((_, marker_len)) = lp_utf16_string_at(bytes, *marker_at) else {
            continue;
        };
        let mut cursor = marker_at + marker_len;
        let Some(optional_name_count) = u32_at(bytes, cursor) else {
            continue;
        };
        if optional_name_count == 0 {
            cursor += 4;
        } else {
            let Some((display_name, display_name_end)) = lp_utf16_bounded(bytes, cursor, 1..=256)
            else {
                continue;
            };
            if display_name.chars().any(char::is_control) {
                continue;
            }
            cursor = display_name_end;
        }
        let Some((selector, selector_len)) = lp_utf16_string_at(bytes, cursor) else {
            continue;
        };
        if !legacy_face_selector_is_valid(selector_kind, &selector) {
            continue;
        }
        cursor += selector_len;
        if bytes.get(cursor..cursor + 4) != Some(&0_f32.to_le_bytes())
            || bytes.get(cursor + 4..cursor + 8) != Some(&1_f32.to_le_bytes())
        {
            continue;
        }

        out.push(FaceAppearanceAssignment {
            face_guid,
            visual_guid: visual.clone(),
            color: Some(color),
        });
    }
    out
}

/// Decode the normalized RGBA carrier of a legacy face assignment.
fn normalized_legacy_face_color(bytes: &[u8], offset: usize) -> Option<Color> {
    let raw = bytes.get(offset..offset + 4 * size_of::<f32>())?;
    let component = |at: usize| Some(f32::from_le_bytes(raw.get(at..at + 4)?.try_into().ok()?));
    let color = Color {
        r: component(0)?,
        g: component(4)?,
        b: component(8)?,
        a: component(12)?,
    };
    [color.r, color.g, color.b, color.a]
        .into_iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
        .then_some(color)
        .filter(|color| color.a == 1.0)
}

/// Decode the selector-name form flag in the legacy twelve-byte carrier.
fn legacy_face_selector_kind(carrier: Option<&[u8]>) -> Option<u8> {
    let carrier = carrier?;
    (carrier.len() == 12
        && carrier.get(0..2) == Some(&[1, 1])
        && carrier.get(2..11) == Some(&[0; 9])
        && matches!(carrier[11], 0 | 1))
    .then_some(carrier[11])
}

/// Validate the selector family selected by the legacy carrier flag.
fn legacy_face_selector_is_valid(kind: u8, selector: &str) -> bool {
    match kind {
        0 => selector.strip_prefix("Prism-").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        }),
        1 => selector.strip_prefix("Prism").is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric())
        }),
        _ => false,
    }
}

/// Decode a face-scoped appearance assignment from the paired-library marker
/// form.
///
/// The assignment envelope ends with the visual token and the two library
/// marker GUIDs. Two lower-case GUIDs precede the tail through fixed carrier
/// gaps; the second is the B-rep face-attribute identity. Other paired-library
/// envelopes do not satisfy this grammar.
fn modern_face_appearance_assignments(
    bytes: &[u8],
    strings: &[(usize, String)],
) -> Vec<FaceAppearanceAssignment> {
    const LP_GUID_BYTES: usize = 4 + GUID_LEN * 2;
    const FIRST_GUID_GAP: usize = 25;
    const FACE_GUID_GAP: usize = 28;

    let mut out = Vec::new();
    for (index, (marker_at, marker)) in strings.iter().enumerate() {
        if marker != APPEARANCE_LIBRARY_ID_PAIR[0]
            || strings
                .get(index + 1)
                .is_none_or(|(_, next)| next != APPEARANCE_LIBRARY_ID_PAIR[1])
        {
            continue;
        }
        let Some((visual_at, visual)) = index.checked_sub(1).and_then(|at| strings.get(at)) else {
            continue;
        };
        let Some((_, visual_len)) = lp_utf16_string_at(bytes, *visual_at) else {
            continue;
        };
        if visual_at.checked_add(visual_len) != Some(*marker_at) || visual_token(visual).is_none() {
            continue;
        }

        let Some((_, first_library_len)) = lp_utf16_string_at(bytes, *marker_at) else {
            continue;
        };
        let Some(second_library_at) = marker_at.checked_add(first_library_len).and_then(|end| {
            let separator_end = end.checked_add(4)?;
            (bytes.get(end..separator_end) == Some(&[0; 4])).then_some(separator_end)
        }) else {
            continue;
        };
        if strings.get(index + 1).map(|(at, _)| *at) != Some(second_library_at) {
            continue;
        }

        let Some(face_end) = visual_at.checked_sub(FACE_GUID_GAP) else {
            continue;
        };
        let Some(face_at) = face_end.checked_sub(LP_GUID_BYTES) else {
            continue;
        };
        let Some((face_guid, face_len)) = lp_utf16_string_at(bytes, face_at) else {
            continue;
        };
        if face_at.checked_add(face_len) != Some(face_end)
            || !is_lowercase_guid(&face_guid)
            || !is_face_to_visual_gap(bytes.get(face_end..*visual_at))
        {
            continue;
        }

        let Some(first_guid_end) = face_at.checked_sub(FIRST_GUID_GAP) else {
            continue;
        };
        let Some(first_guid_at) = first_guid_end.checked_sub(LP_GUID_BYTES) else {
            continue;
        };
        let Some((first_guid, first_guid_len)) = lp_utf16_string_at(bytes, first_guid_at) else {
            continue;
        };
        if first_guid_at.checked_add(first_guid_len) != Some(first_guid_end)
            || !is_lowercase_guid(&first_guid)
            || !is_first_guid_to_face_gap(bytes.get(first_guid_end..face_at))
        {
            continue;
        }
        out.push(FaceAppearanceAssignment {
            face_guid,
            visual_guid: visual.clone(),
            color: None,
        });
    }
    out
}

/// Validate the fixed carrier gap after the first lower-case GUID of a paired
/// face-appearance envelope. Its leading eight-byte field is not framing; the
/// remaining bytes are invariant.
fn is_first_guid_to_face_gap(gap: Option<&[u8]>) -> bool {
    gap.is_some_and(|gap| {
        gap.len() == 25
            && gap.get(8..16) == Some(&[0; 8])
            && gap.get(16..20) == Some(&1_u32.to_le_bytes())
            && gap.get(20..22) == Some(&[1, 1])
            && gap.get(22..25) == Some(&[0; 3])
    })
}

/// Validate the fixed presentation tail between the B-rep face GUID and the
/// visual token.
fn is_face_to_visual_gap(gap: Option<&[u8]>) -> bool {
    gap.is_some_and(|gap| {
        gap.len() == 28
            && gap.get(0..12) == Some(&[0; 12])
            && gap.get(12..16) == Some(&1_f32.to_le_bytes())
            && gap.get(16..18) == Some(&[1, 1])
            && gap.get(18..28) == Some(&[0; 10])
    })
}

/// Whether the complete value is a lower-case hyphenated hexadecimal GUID.
fn is_lowercase_guid(value: &str) -> bool {
    value.len() == GUID_LEN
        && is_guid_prefix(value)
        && value[..GUID_LEN]
            .bytes()
            .all(|byte| !byte.is_ascii_uppercase())
}

/// Decode legacy body-presentation records that identify their body through a
/// browser-node GUID rather than a typed body owner.
///
/// The terminating visual marker is shared with face-presentation records.
/// A record is body-owned only when exactly one GUID in its bounded prefix
/// resolves through a browser-node record to one Design entity suffix.
pub(crate) fn browser_body_appearances(bytes: &[u8]) -> Vec<(u64, String)> {
    let nodes = crate::design::decode::body::browser_node_entities(bytes);
    let strings = lp_utf16_strings(bytes);
    let mut out = Vec::new();
    for (index, (_, marker)) in strings.iter().enumerate() {
        if index == 0 {
            continue;
        }
        if marker != APPEARANCE_LIBRARY_ID {
            continue;
        }
        let visual = &strings[index - 1].1;
        if visual_token(visual).is_none() {
            continue;
        }
        if let Some(entity_suffix) = body_node_candidate(&strings, index, &nodes) {
            out.push((entity_suffix, visual.clone()));
        }
    }
    let mut seen = std::collections::HashSet::new();
    out.retain(|binding| seen.insert(binding.clone()));
    out
}

fn body_node_candidate(
    strings: &[(usize, String)],
    marker_index: usize,
    nodes: &std::collections::HashMap<String, u64>,
) -> Option<u64> {
    const APPEARANCE_MARKER: &str = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
    let marker = strings[..marker_index]
        .iter()
        .rposition(|(_, value)| value == APPEARANCE_MARKER)?;
    let start = marker.saturating_sub(3);
    let candidates = strings[start..marker_index.saturating_sub(1)]
        .iter()
        .filter_map(|(_, candidate)| nodes.get(&candidate.to_ascii_lowercase()).copied())
        .collect::<std::collections::HashSet<_>>();
    (candidates.len() == 1).then(|| {
        *candidates
            .iter()
            .next()
            .expect("one browser-node candidate was established")
    })
}

fn bind_bodies(
    appearances: &[Appearance],
    assignments: &[DesignMaterialAssignment],
    act_channels: &std::collections::HashMap<u64, BTreeMap<String, String>>,
    object_types: &std::collections::HashMap<u64, String>,
    body_bindings: &[DesignBodyBinding],
) -> Result<Vec<AppearanceBinding>, CodecError> {
    let mut out = Vec::new();
    for assignment in assignments {
        let Some(body) = resolved_body_for_map_pair(
            body_bindings,
            &assignment.id,
            assignment.asm_body_key,
            assignment.asm_body_key_offset,
            assignment.entity_suffix,
            assignment.entity_suffix_offset,
        )?
        else {
            continue;
        };
        let Some(appearance) = appearance_for_assignment(appearances, assignment)? else {
            continue;
        };
        out.push(AppearanceBinding {
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
        });
    }
    Ok(out)
}

/// Resolve one Design assignment to a unique appearance record.
///
/// The complete visual token is authoritative. A present preset name is a
/// secondary identity only when no appearance carries that token.
pub(crate) fn appearance_for_assignment<'a>(
    appearances: &'a [Appearance],
    assignment: &DesignMaterialAssignment,
) -> Result<Option<&'a Appearance>, CodecError> {
    appearance_for_visual_token(
        appearances,
        &assignment.visual_guid,
        assignment.visual_preset.as_deref(),
    )
}

/// Resolve one complete serialized visual token to a unique appearance.
///
/// A preset name is an optional fallback for assignments whose visual token
/// names no decoded asset. Absence of a preset supplies no fallback identity.
pub(crate) fn appearance_for_visual_token<'a>(
    appearances: &'a [Appearance],
    serialized_token: &str,
    fallback_name: Option<&str>,
) -> Result<Option<&'a Appearance>, CodecError> {
    if visual_token(serialized_token).is_none() {
        return Err(CodecError::Malformed(
            "F3D appearance assignment has a malformed visual token".into(),
        ));
    }
    let exact = unique_appearance(
        appearances.iter().filter(|appearance| {
            appearance
                .visual_guid
                .as_deref()
                .is_some_and(|token| visual_tokens_match(token, serialized_token))
        }),
        "visual token",
    )?;
    if exact.is_some() {
        return Ok(exact);
    }
    let Some(name) = fallback_name else {
        return Ok(None);
    };
    unique_appearance(
        appearances
            .iter()
            .filter(|appearance| appearance.name.as_deref() == Some(name)),
        "visual preset",
    )
}

fn unique_appearance<'a>(
    mut matches: impl Iterator<Item = &'a Appearance>,
    identity: &str,
) -> Result<Option<&'a Appearance>, CodecError> {
    let Some(appearance) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "F3D {identity} matches multiple appearance assets"
        )));
    }
    Ok(Some(appearance))
}

/// Resolve one material owner through its exact ordered body-map pair.
///
/// ASM keys are local to the pair's BREP basename. The already-resolved native
/// binding therefore remains authoritative; reconstructing a document-global
/// key map would discard that namespace.
fn resolved_body_for_map_pair(
    body_bindings: &[DesignBodyBinding],
    owner_id: &str,
    asm_body_key: u64,
    asm_body_key_offset: u64,
    entity_suffix: u64,
    entity_suffix_offset: u64,
) -> Result<Option<BodyId>, CodecError> {
    let owner_stream = crate::ids::native_stream(owner_id).ok_or_else(|| {
        CodecError::Malformed(format!(
            "F3D material owner has no native stream: {owner_id}"
        ))
    })?;
    let mut matches = body_bindings.iter().filter(|binding| {
        crate::ids::native_stream(&binding.id) == Some(owner_stream)
            && binding.asm_body_key == asm_body_key
            && binding.asm_body_key_offset == asm_body_key_offset
            && binding.entity_suffix == entity_suffix
            && binding.entity_suffix_offset == entity_suffix_offset
    });
    let Some(binding) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "F3D material owner {owner_id} matches multiple exact body-map pairs"
        )));
    }
    Ok(binding.body.clone())
}

fn decode_design_object_types(
    scan: &ContainerScan,
) -> Result<std::collections::HashMap<u64, String>, CodecError> {
    let mut out = std::collections::HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, role::METASTREAM))
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
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_act_stream(entry))
    {
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

/// Select the sole ordered body-map pair carrying one material owner's Design
/// entity suffix. More than one pair leaves the owner ambiguous.
fn unique_body_map_pair<'a>(
    body_map: &'a [crate::design::decode::body::BodyBinding],
    entity_suffix: u64,
    owner_kind: &str,
) -> Result<Option<&'a crate::design::decode::body::BodyBinding>, CodecError> {
    let mut matches = body_map
        .iter()
        .filter(|binding| binding.entity_suffix == entity_suffix);
    let Some(binding) = matches.next() else {
        return Ok(None);
    };
    if matches.next().is_some() {
        return Err(CodecError::Malformed(format!(
            "F3D {owner_kind} entity {entity_suffix} matches multiple body-map pairs"
        )));
    }
    Ok(Some(binding))
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

/// Decode the fixed source-less layouts emitted by [`encode_protein`]. Native
/// Protein assets package schemas and use the schema-driven path instead.
fn decode_fixed_logical_records(frames: &[crate::protein::RecordFrame]) -> Vec<Appearance> {
    frames
        .iter()
        .filter_map(|frame| decode_fixed_record(&frame.bytes))
        .collect()
}

/// Decode one record of a fixed source-less layout.
///
/// The schema set here is exactly the set [`encode_protein`] emits, and the
/// offsets are that encoder's own layout rather than a property order stated by
/// the format. A record carrying any other schema declines: its member offsets
/// follow from the schema packaged beside it, which the schema-driven path
/// reads. That covers the `interior_model` subtypes with no fixed layout here,
/// `PrismLayeredSchema` and `PrismWoodSchema`, whose colour offset must not be
/// assumed from the opaque, metal, or transparent layouts.
fn decode_fixed_record(record: &[u8]) -> Option<Appearance> {
    let mut position = RECORD_MARKER.len();
    let schema = take_lp_utf8(record, &mut position)?;
    let guid = take_lp_utf8(record, &mut position)?;
    let base = take_lp_utf8(record, &mut position)?;
    let asset_lib_id = take_lp_utf8(record, &mut position)?;
    let color = match schema.as_str() {
        "GenericSchema" => fixed_rgba(
            record,
            position + 112 + generic_connection_delta(record, position)?,
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
        let delta = generic_connection_delta(record, position)?;
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
        library_id: library_id(&asset_lib_id),
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

fn generic_connection_delta(record: &[u8], value_block: usize) -> Option<usize> {
    let slot = value_block.checked_add(102)?;
    match record.get(slot) {
        Some(0) => Some(0),
        Some(1) if slot + 6 <= record.len() => {
            let count = u32::from_le_bytes(
                record[slot + 2..slot + 6]
                    .try_into()
                    .expect("invariant: record[slot+2..slot+6] is a 4-byte slice"),
            ) as usize;
            if count > 8 {
                return None;
            }
            let mut position = slot + 6;
            for _ in 0..count {
                let length_bytes = record.get(position..position + 4)?;
                let length = u32::from_le_bytes(length_bytes.try_into().expect(
                    "invariant: length_bytes is a 4-byte slice from bytes.get(range) of length 4",
                )) as usize;
                position += 4;
                record.get(position..position + length)?;
                position += length;
            }
            position.checked_sub(slot + 1)
        }
        _ => None,
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
    fn raw_body_map_pair(
        asm_key_offset: usize,
        entity_suffix: u64,
    ) -> crate::design::decode::body::BodyBinding {
        crate::design::decode::body::BodyBinding {
            blob_name: "BREP.synthetic.smbh".into(),
            blob_name_offset: asm_key_offset + 32,
            pair_count: 2,
            pair_ordinal: 0,
            asm_key: 7,
            asm_key_offset,
            entity_suffix,
            entity_suffix_offset: asm_key_offset + 8,
        }
    }

    fn resolved_body_binding(
        stream: &str,
        asm_key_offset: u64,
        entity_suffix: u64,
        blob_name: &str,
        body: &str,
    ) -> crate::records::DesignBodyBinding {
        crate::records::DesignBodyBinding {
            id: crate::ids::native_design_body_binding_id(stream, asm_key_offset),
            stream: stream.into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 7,
            asm_body_key_offset: asm_key_offset,
            entity_suffix,
            entity_suffix_offset: asm_key_offset + 8,
            blob_name: blob_name.into(),
            blob_name_offset: asm_key_offset + 32,
            body: Some(cadmpeg_ir::ids::BodyId(body.into())),
        }
    }

    #[test]
    fn material_owner_rejects_more_than_one_pair_for_its_entity_suffix() {
        let body_map = [raw_body_map_pair(25, 100), raw_body_map_pair(41, 100)];
        let Err(error) = super::unique_body_map_pair(&body_map, 100, "material assignment") else {
            panic!("one Design entity must not select two map pairs")
        };
        assert!(error
            .to_string()
            .contains("matches multiple body-map pairs"));
    }

    #[test]
    fn equal_keys_in_different_brep_namespaces_resolve_by_exact_map_pair() {
        let stream = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let first = resolved_body_binding(
            stream,
            25,
            100,
            "BREP.first.smbh",
            "f3d:brep/first/brep:entity#1",
        );
        let second_body = cadmpeg_ir::ids::BodyId("f3d:brep/second/brep:entity#1".into());
        let second = resolved_body_binding(stream, 125, 200, "BREP.second.smbh", &second_body.0);
        let owner = crate::ids::native_scoped_id(stream, "material-assignment", 500);
        let visual_guid = "11111111-2222-3333-4444-555555555555";
        let appearance = cadmpeg_ir::appearance::Appearance {
            id: cadmpeg_ir::ids::AppearanceId("f3d:appearance#second".into()),
            name: None,
            asset_guid: Some(visual_guid.into()),
            library_id: None,
            visual_guid: Some(visual_guid.into()),
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        };
        let assignment = crate::records::DesignMaterialAssignment {
            id: owner,
            asm_body_key: 7,
            asm_body_key_offset: 125,
            entity_suffix: 200,
            entity_suffix_offset: 133,
            entity_id: "0_200".into(),
            entity_id_offset: 500,
            visual_guid: visual_guid.into(),
            visual_guid_offset: 600,
            physical_token: None,
            physical_token_offset: None,
            visual_preset: None,
            visual_preset_offset: None,
        };
        let projected = super::bind_bodies(
            &[appearance],
            &[assignment],
            &std::collections::HashMap::new(),
            &std::collections::HashMap::new(),
            &[first, second],
        )
        .expect("blob-qualified material binding");
        let [binding] = projected.as_slice() else {
            panic!("one appearance binding expected")
        };
        assert_eq!(
            binding.target,
            cadmpeg_ir::appearance::AppearanceTarget::Body(second_body)
        );
    }

    #[test]
    fn presetless_assignment_matches_only_its_visual_guid() {
        let appearance_guid = "11111111-2222-3333-4444-555555555555";
        let mut appearance = cadmpeg_ir::appearance::Appearance {
            id: cadmpeg_ir::ids::AppearanceId("f3d:appearance#catalog".into()),
            name: None,
            asset_guid: Some(appearance_guid.into()),
            library_id: None,
            visual_guid: Some(appearance_guid.into()),
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        };
        let mut assignment = crate::records::DesignMaterialAssignment {
            id: "f3d:design:material-assignment#1".into(),
            asm_body_key: 7,
            asm_body_key_offset: 25,
            entity_suffix: 100,
            entity_suffix_offset: 33,
            entity_id: "0_100".into(),
            entity_id_offset: 500,
            visual_guid: "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into(),
            visual_guid_offset: 600,
            physical_token: None,
            physical_token_offset: None,
            visual_preset: None,
            visual_preset_offset: None,
        };

        assert!(
            super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
                .expect("valid preset-less assignment")
                .is_none()
        );

        assignment.visual_guid = appearance_guid.into();
        assert!(
            super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
                .expect("exact visual-token assignment")
                .is_some()
        );

        assignment.visual_guid = "AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE".into();
        assignment.visual_preset = Some("Prism-017".into());
        appearance.name = Some("Prism-017".into());
        assert!(
            super::appearance_for_assignment(std::slice::from_ref(&appearance), &assignment)
                .expect("present preset-name fallback")
                .is_some()
        );
    }

    #[test]
    fn complete_visual_token_selects_one_revision_record() {
        let base_token = "11111111-2222-3333-4444-555555555555";
        let revised_token = "11111111-2222-3333-4444-555555555555_Post2015";
        let appearance = |id: &str, token: &str| cadmpeg_ir::appearance::Appearance {
            id: cadmpeg_ir::ids::AppearanceId(id.into()),
            name: None,
            asset_guid: Some(token.into()),
            library_id: None,
            visual_guid: Some(token.into()),
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        };
        let appearances = [
            appearance("f3d:appearance#base", base_token),
            appearance("f3d:appearance#revised", revised_token),
        ];

        let selected = super::appearance_for_visual_token(&appearances, revised_token, None)
            .expect("unique complete visual token")
            .expect("revised appearance exists");
        assert_eq!(selected.id.as_str(), "f3d:appearance#revised");

        let duplicates = [
            appearance("f3d:appearance#first", revised_token),
            appearance("f3d:appearance#second", revised_token),
        ];
        assert!(matches!(
            super::appearance_for_visual_token(&duplicates, revised_token, None),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn visual_preset_fallback_requires_one_record() {
        let appearance = |id: &str| cadmpeg_ir::appearance::Appearance {
            id: cadmpeg_ir::ids::AppearanceId(id.into()),
            name: Some("Prism-017".into()),
            asset_guid: None,
            library_id: None,
            visual_guid: None,
            physical_token: None,
            schema: None,
            category: None,
            base_color: None,
            properties: std::collections::BTreeMap::new(),
            textures: Vec::new(),
        };
        let appearances = [
            appearance("f3d:appearance#first"),
            appearance("f3d:appearance#second"),
        ];

        assert!(matches!(
            super::appearance_for_visual_token(
                &appearances,
                "11111111-2222-3333-4444-555555555555",
                Some("Prism-017"),
            ),
            Err(cadmpeg_core::CodecError::Malformed(_))
        ));
    }

    #[test]
    fn generic_connection_delta_rejects_unknown_and_truncated_forms() {
        let mut record = vec![0; 120];
        record[102] = 2;
        assert_eq!(super::generic_connection_delta(&record, 0), None);

        record[102] = 1;
        record[104..108].copy_from_slice(&1u32.to_le_bytes());
        record[108..112].copy_from_slice(&16u32.to_le_bytes());
        assert_eq!(super::generic_connection_delta(&record, 0), None);
    }

    fn distance_record(unit: u32, value: f64) -> crate::protein::DecodedRecord {
        crate::protein::DecodedRecord {
            logical_offset: 0,
            schema: "TestSchema".into(),
            guid: String::new(),
            base: String::new(),
            asset_lib_id: String::new(),
            properties: std::collections::BTreeMap::from([(
                "test_Depth".to_owned(),
                crate::protein::DecodedProperty {
                    value_offset: 0,
                    value: crate::protein::PropertyValue::Distance { unit, value },
                    connections: Vec::new(),
                },
            )]),
        }
    }

    #[test]
    fn decoded_color_requires_finite_normalized_channels() {
        assert!(super::decoded_color([0.0, 0.25, 0.5, 1.0]).is_some());
        for invalid in [f64::NAN, f64::INFINITY, -0.01, 1.01] {
            assert!(super::decoded_color([invalid, 0.25, 0.5, 1.0]).is_none());
        }
    }

    /// The three length tags of the Distance quantity class each convert to
    /// the IR's millimetres. `0x200e` is millimetre, not centimetre.
    #[test]
    fn distance_tags_convert_to_millimetres() {
        for (unit, value, expected) in [(0x2016, 1.0, 25.4), (0x200e, 0.5, 0.5), (0x200d, 0.5, 5.0)]
        {
            let record = distance_record(unit, value);
            assert_eq!(super::distance_property(&record, "Depth"), Some(expected));
        }
    }

    #[test]
    fn schema_primary_colour_wins_over_rival_colour_members() {
        for (schema, primary_id) in [
            ("GenericSchema", "generic_diffuse"),
            ("MetalSchema", "metal_color"),
            ("MetallicPaintSchema", "metallicpaint_base_color"),
            ("PlasticVinylSchema", "plasticvinyl_color"),
            ("PrismLayeredSchema", "layered_diffuse"),
            ("PrismMetalSchema", "metal_f0"),
            ("PrismOpaqueSchema", "opaque_albedo"),
            ("PrismTransparentSchema", "transparent_color"),
            ("PrismWoodSchema", "surface_albedo"),
        ] {
            let mut properties = std::collections::BTreeMap::from([
                color_property("common_Tint_color", [0.75, 0.75, 0.75, 1.0]),
                color_property("surface_albedo", [0.5, 0.5, 0.5, 1.0]),
                (
                    "common_Tint_toggle".to_owned(),
                    crate::protein::DecodedProperty {
                        value_offset: 0,
                        value: crate::protein::PropertyValue::Boolean(false),
                        connections: Vec::new(),
                    },
                ),
            ]);
            properties.insert(
                primary_id.to_owned(),
                crate::protein::DecodedProperty {
                    value_offset: 0,
                    value: crate::protein::PropertyValue::Color([0.125, 0.25, 0.375, 1.0]),
                    connections: Vec::new(),
                },
            );
            let record = appearance_record(schema, properties);
            assert_eq!(
                super::appearance_base_color(&record).map(|color| color.g),
                Some(0.25),
                "{schema} selects {primary_id}"
            );
        }
    }

    #[test]
    fn enabled_common_tint_replaces_the_schema_primary_colour() {
        let mut properties = std::collections::BTreeMap::from([
            color_property("opaque_albedo", [0.125, 0.25, 0.375, 1.0]),
            color_property("surface_albedo", [0.5, 0.5, 0.5, 1.0]),
            color_property("common_Tint_color", [0.75, 0.625, 0.5, 1.0]),
        ]);
        properties.insert(
            "common_Tint_toggle".to_owned(),
            crate::protein::DecodedProperty {
                value_offset: 0,
                value: crate::protein::PropertyValue::Boolean(true),
                connections: Vec::new(),
            },
        );
        let record = appearance_record("PrismOpaqueSchema", properties);
        assert_eq!(
            super::appearance_base_color(&record).map(|color| color.g),
            Some(0.625)
        );
    }

    fn color_property(id: &str, color: [f64; 4]) -> (String, crate::protein::DecodedProperty) {
        (
            id.to_owned(),
            crate::protein::DecodedProperty {
                value_offset: 0,
                value: crate::protein::PropertyValue::Color(color),
                connections: Vec::new(),
            },
        )
    }

    fn appearance_record(
        schema: &str,
        properties: std::collections::BTreeMap<String, crate::protein::DecodedProperty>,
    ) -> crate::protein::DecodedRecord {
        crate::protein::DecodedRecord {
            logical_offset: 0,
            schema: schema.to_owned(),
            guid: "11111111-2222-3333-4444-555555555555".to_owned(),
            base: "Prism-001".to_owned(),
            asset_lib_id: String::new(),
            properties,
        }
    }

    /// A Distance whose tag names a quantity other than length has no
    /// millimetre reading and must not be silently taken as one.
    #[test]
    fn a_non_length_distance_tag_yields_no_value() {
        let record = distance_record(0x0002_1008, 1.0);
        assert_eq!(super::distance_property(&record, "Depth"), None);
    }
}
