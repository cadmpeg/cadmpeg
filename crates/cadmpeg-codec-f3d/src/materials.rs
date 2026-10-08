// SPDX-License-Identifier: Apache-2.0
//! Decode Fusion `.protein` appearance assets and bind them to B-rep bodies.
//!
//! Material and appearance semantics are defined in [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
//! [`decode`] reads appearance records without resolving body bindings.
//! [`decode_with_body_bindings`] joins Protein assets, Design assignments, ACT
//! channels, and blob-qualified Design body-map bindings through the
//! design-entity join backbone in
//! [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).

use cadmpeg_core::convert::f32_from_f64;
use cadmpeg_core::decode::{index_from_u32, u64_from_index};

use crate::records::references::DesignVisualToken;
use cadmpeg_core::container::ContainerRole;

use std::collections::BTreeMap;
use std::io::{Cursor, Write};

use crate::records::{bodies::DesignBodyBinding, references::DesignMaterialAssignment};
use cadmpeg_container::ArchiveSnapshot;
use cadmpeg_core::decode::{bounded_len, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::appearance::{
    Appearance, AppearanceBinding, AppearanceTarget, BumpMap, TextureMap2d, TextureRef,
};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Color;
use cadmpeg_protein::appearance::{
    is_physical_schema, neutral_property_name, texture_asset, TextureAssetResult,
};
use cadmpeg_protein::{
    CONTINUATION_MARKER, PAGE_SIZE, RECORD_MARKER, STREAM_HEADER_LEN, TERMINAL_MARKER,
};

use crate::bytes::{
    is_guid_prefix, lp_utf16_bounded_charged, skip_lp_u32_bytes, take_lp_utf8, take_lp_utf8_charged,
};
use crate::container::ContainerScan;
use crate::design::presentation::{
    APPEARANCE_LIBRARY_ID, GUID_LEN, MODERN_APPEARANCE_LIBRARY_IDS as APPEARANCE_LIBRARY_ID_PAIR,
};
/// The `AssetLibID` [`encode_protein`] writes for an appearance that names no
/// library. A stored library identifier is a library GUID or a library path;
/// the null GUID names neither.
const NO_ASSET_LIB_ID: &str = "00000000-0000-0000-0000-000000000000";

/// The library identifier an `InstanceProperties` record stores, when it names
/// a library.
fn library_id(ctx: &DecodeContext<'_>, asset_lib_id: &str) -> Result<Option<String>, CodecError> {
    if asset_lib_id.is_empty() || asset_lib_id == NO_ASSET_LIB_ID {
        return Ok(None);
    }
    Ok(Some(ctx.copy_retained_text(
        asset_lib_id,
        "copy F3D appearance library ID",
    )?))
}

fn copy_act_channels(
    ctx: &DecodeContext<'_>,
    channels: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<String, String>, CodecError> {
    let mut copied = BTreeMap::new();
    if let Some(channels) = channels {
        for (name, guid) in channels {
            let name = ctx.copy_retained_text(name, "copy F3D ACT channel name")?;
            let guid = ctx.copy_retained_text(guid, "copy F3D ACT channel GUID")?;
            ctx.insert_btree_map(&mut copied, name, guid, "copy F3D ACT channel map")?;
        }
    }
    Ok(copied)
}

fn named_act_channels(
    ctx: &DecodeContext<'_>,
    owner: std::fmt::Arguments<'_>,
    channels: Option<&BTreeMap<String, String>>,
) -> Result<BTreeMap<cadmpeg_core::text::NonBlankString, String>, CodecError> {
    let copied = copy_act_channels(ctx, channels)?;
    Ok(cadmpeg_core::text::named_entries_for_decode(
        ctx, owner, copied,
    )?)
}

/// A counted printable-ASCII field of 1 to 64 bytes, borrowed from `bytes`;
/// callers copy it only when they keep it.
fn lp_ascii_printable<'a>(
    ctx: &DecodeContext<'_>,
    bytes: &'a [u8],
    at: usize,
) -> Result<Option<(&'a str, usize)>, CodecError> {
    let Some(length) = View::u32_le_at(bytes, at).and_then(|value| usize::try_from(value).ok())
    else {
        return Ok(None);
    };
    if !(1..=64).contains(&length) {
        return Ok(None);
    }
    let Some(start) = at.checked_add(4) else {
        return Ok(None);
    };
    let Some(end) = start.checked_add(length) else {
        return Ok(None);
    };
    let Some(raw) = bytes.get(start..end) else {
        return Ok(None);
    };
    if !ctx.all_by(
        raw,
        |byte| Ok((0x20..0x7f).contains(byte)),
        "validate F3D printable ASCII bytes",
    )? {
        return Ok(None);
    }
    let text = ctx
        .validate_utf8(raw, "validate F3D printable ASCII UTF-8")?
        .map_err(|_| CodecError::Malformed("F3D printable ASCII validation failed".into()))?;
    Ok(Some((text, end)))
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
                logical[value_block + 175..value_block + 183]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
            if let Some(value) = appearance.properties.get("refraction_index") {
                logical[value_block + 197..value_block + 201].copy_from_slice(b"\x0c\x00\x00\x00");
                logical[value_block + 201..value_block + 209]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
        }
        "PrismOpaqueSchema" | "PrismMetalSchema" => {
            logical.resize(value_block + 96, 0);
            write_color(&mut logical, value_block + 8, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("surface_roughness") {
                logical[value_block + 64..value_block + 68].copy_from_slice(b"\x0e\x20\x00\x00");
                logical[value_block + 68..value_block + 76]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
        }
        "PrismTransparentSchema" => {
            logical.resize(value_block + 177, 0);
            write_color(&mut logical, value_block + 121, appearance.base_color)?;
            if let Some(value) = appearance.properties.get("refraction_index") {
                logical[value_block + 169..value_block + 177]
                    .copy_from_slice(&value.get().to_le_bytes());
            }
        }
        "PhysMatSchema"
        | "StructuralMetalSchema"
        | "StructuralPlasticSchema"
        | "ThermalSolidSchema" => logical.resize(value_block + 8, 0),
        _ => {
            return Err(CodecError::NotImplemented(format!(
                "source-less Protein schema {schema} is unsupported"
            )));
        }
    }
    let instance = page_logical(&logical)?;
    let mut catalog = RECORD_MARKER.to_vec();
    push_lp(&mut catalog, schema)?;
    catalog.push(0);
    push_lp(&mut catalog, name)?;
    push_lp(&mut catalog, name)?;
    catalog.extend_from_slice(&2_u32.to_le_bytes());
    push_lp(
        &mut catalog,
        appearance.category.as_deref().unwrap_or("Generated"),
    )?;
    push_lp(&mut catalog, "Default")?;
    push_lp(&mut catalog, "")?;
    catalog.extend_from_slice(&0_u32.to_le_bytes());
    catalog.extend_from_slice(&1_u32.to_le_bytes());
    push_lp(&mut catalog, "")?;
    let catalog = page_logical(&catalog)?;
    let options = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("AssetData/InstanceProperties.bin", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create Protein instance: {error}"))
        })?;
    zip.write_all(&instance)?;
    zip.start_file("AssetData/DefinitionIteratorProperties.bin", options)
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot create Protein catalog: {error}"))
        })?;
    zip.write_all(&catalog)?;
    Ok(zip
        .finish()
        .map_err(|error| {
            CodecError::malformed(format_args!("cannot finish Protein asset: {error}"))
        })?
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
    for (ordinal, value) in [color.r(), color.g(), color.b(), color.a()]
        .into_iter()
        .enumerate()
    {
        let at = offset + ordinal * 8;
        out[at..at + 8].copy_from_slice(&f64::from(value).to_le_bytes());
    }
    Ok(())
}

fn page_logical(logical: &[u8]) -> Result<Vec<u8>, CodecError> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(
        &u32::try_from(PAGE_SIZE)
            .map_err(|_| CodecError::malformed("Protein page size exceeds u32"))?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let first = logical.len().min(PAGE_SIZE - 4);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&logical[..first]);
    bytes.resize(STREAM_HEADER_LEN + PAGE_SIZE, 0);
    let mut rest = &logical[first..];
    while rest.len() > PAGE_SIZE - 8 {
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(CONTINUATION_MARKER);
        bytes.extend_from_slice(&rest[..PAGE_SIZE - 8]);
        rest = &rest[PAGE_SIZE - 8..];
    }
    if !rest.is_empty() {
        bytes.extend_from_slice(TERMINAL_MARKER);
        let length = u16::try_from(rest.len())
            .map_err(|_| CodecError::Malformed("Protein tail page exceeds u16::MAX".into()))?;
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes.extend_from_slice(&0u16.to_le_bytes());
        bytes.extend_from_slice(rest);
        let end = STREAM_HEADER_LEN + (bytes.len() - STREAM_HEADER_LEN).next_multiple_of(PAGE_SIZE);
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
    notes: &mut Vec<String>,
) -> Result<(Vec<u8>, std::collections::BTreeSet<String>), CodecError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(protein)).map_err(|error| {
        CodecError::malformed(format_args!("cannot open nested Protein ZIP: {error}"))
    })?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut patched = std::collections::BTreeSet::new();
    let mut total_inflated = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            CodecError::malformed(format_args!("cannot read nested Protein entry: {error}"))
        })?;
        let name = entry.name().to_owned();
        let options = crate::zip_write::file_options(entry.compression());
        let declared_size = entry.size();
        total_inflated = total_inflated.checked_add(declared_size).ok_or_else(|| {
            CodecError::Malformed("Protein ZIP total inflated size overflows u64".into())
        })?;
        if total_inflated > crate::container::MAX_ARCHIVE_BYTES {
            return Err(CodecError::malformed(format_args!(
                "Protein ZIP entries declare {total_inflated} inflated bytes; total limit is {}",
                crate::container::MAX_ARCHIVE_BYTES
            )));
        }
        let mut bytes = crate::container::read_entry_bounded(&mut entry, declared_size, &name)?;
        if name.ends_with("AssetData/InstanceProperties.bin") {
            patch_instance_colors(protein, &mut bytes, edits, &mut patched, notes)?;
        }
        zip.start_file(name, options).map_err(|error| {
            CodecError::malformed(format_args!("cannot write nested Protein entry: {error}"))
        })?;
        zip.write_all(&bytes)?;
    }
    let bytes = zip
        .finish()
        .map_err(|error| CodecError::malformed(format_args!("cannot finish Protein ZIP: {error}")))?
        .into_inner();
    Ok((bytes, patched))
}

fn patch_instance_colors(
    protein: &[u8],
    bytes: &mut [u8],
    edits: &BTreeMap<String, ProteinAppearanceEdit>,
    patched: &mut std::collections::BTreeSet<String>,
    notes: &mut Vec<String>,
) -> Result<(), CodecError> {
    let admission = cadmpeg_protein::admission::StandardAdmission;
    let frames = cadmpeg_protein::framing::record_frames_admitted(admission, bytes)?;
    let catalog = cadmpeg_protein::SchemaCatalog::load(admission, protein)?;
    let schema_driven = catalog.is_some();
    let decoded = if let Some(mut catalog) = catalog {
        let outcome =
            cadmpeg_protein::decode_frames_admitted(admission, &mut catalog, frames.frames())?;
        notes.extend(outcome.rejected.iter().map(|rejected| {
            format!(
                "Protein record {} rejected: {}",
                rejected.ordinal, rejected.detail
            )
        }));
        outcome.records
    } else {
        Vec::new()
    };
    for frame in frames.frames() {
        let record = frame.bytes();
        let mut position = RECORD_MARKER.len();
        let schema = take_lp_utf8(record, &mut position).ok_or_else(|| {
            CodecError::Malformed("Protein appearance schema is truncated".into())
        })?;
        let guid = take_lp_utf8(record, &mut position)
            .ok_or_else(|| CodecError::Malformed("Protein appearance GUID is truncated".into()))?;
        skip_lp_u32_bytes(record, &mut position).ok_or_else(|| {
            CodecError::Malformed(
                "Protein appearance record is truncated at the string after its GUID".into(),
            )
        })?;
        skip_lp_u32_bytes(record, &mut position).ok_or_else(|| {
            CodecError::Malformed(
                "Protein appearance record is truncated at the second string after its GUID".into(),
            )
        })?;
        let Some(edit) = edits.get(&guid) else {
            continue;
        };
        let decoded_record = if schema_driven {
            Some(
                decoded
                    .iter()
                    .find(|decoded| {
                        decoded.logical_offset == frame.logical_offset()
                            && decoded.schema == schema
                            && decoded.guid == guid
                    })
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
                            "Protein appearance {guid} has no decoded schema record"
                        ))
                    })?,
            )
        } else {
            None
        };
        if let Some(color) = edit.color {
            let relative = if let Some(decoded_record) = decoded_record {
                let property_id = appearance_base_color_property_id(
                    decoded_record.schema.as_str(),
                    || {
                        Ok::<_, CodecError>(matches!(
                            decoded_record
                                .properties
                                .get("common_Tint_toggle")
                                .and_then(|property| property.value()),
                            Some(cadmpeg_protein::property::PropertyValue::Boolean(true))
                        ))
                    },
                    || Ok(decoded_record.properties.contains_key("surface_albedo")),
                )?
                .ok_or_else(|| {
                    CodecError::malformed(format_args!(
                        "Protein appearance {guid} has no schema-selected color carrier"
                    ))
                })?;
                let property = decoded_record
                    .properties
                    .get(property_id)
                    .filter(|property| {
                        matches!(
                            property.value(),
                            Some(cadmpeg_protein::property::PropertyValue::Color(_))
                        )
                    })
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
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
                        )));
                    }
                }
            };
            for (ordinal, value) in [color.r(), color.g(), color.b(), color.a()]
                .into_iter()
                .enumerate()
            {
                patch_logical_f64(
                    bytes,
                    frame.logical_offset() + relative + ordinal * 8,
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
                        )));
                    }
                };
                decoded_record
                    .properties
                    .get(property_id)
                    .filter(|property| {
                        matches!(
                            property.value(),
                            Some(cadmpeg_protein::property::PropertyValue::Float(_))
                        )
                    })
                    .map(|property| property.value_offset)
                    .ok_or_else(|| {
                        CodecError::malformed(format_args!(
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
                    ("PrismOpaqueSchema", "surface_roughness") => record
                        .get(position..)
                        .and_then(|window| memchr::memmem::find(window, b"\x0e\x20\x00\x00"))
                        .map(|relative| position + relative)
                        .map(|marker| marker + 4)
                        .ok_or_else(|| {
                            CodecError::Malformed("Protein roughness carrier is absent".into())
                        })?,
                    ("PrismTransparentSchema", "refraction_index") => position + 169,
                    _ => {
                        return Err(CodecError::NotImplemented(format!(
                            "Protein schema {schema} property {name} has no writable carrier"
                        )));
                    }
                }
            };
            patch_logical_f64(bytes, frame.logical_offset() + relative, *value)?;
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
    for (index, page) in bytes
        .get(STREAM_HEADER_LEN..)?
        .chunks_exact(PAGE_SIZE)
        .enumerate()
    {
        let (physical_in_page, length) = if page.get(4..8) == Some(RECORD_MARKER) {
            (4, PAGE_SIZE - 4)
        } else if page.get(4..8) == Some(CONTINUATION_MARKER) {
            (8, PAGE_SIZE - 8)
        } else if page.get(0..4) == Some(TERMINAL_MARKER) {
            (8, usize::from(View::u16_le_at(page, 4)?))
        } else {
            return None;
        };
        if logical_offset < logical_start + length {
            return Some(
                STREAM_HEADER_LEN + index * PAGE_SIZE + physical_in_page + logical_offset
                    - logical_start,
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
pub(crate) struct DecodedMaterials {
    /// Merged appearance records, deduplicated by [`AppearanceId`].
    pub(crate) appearances: Vec<Appearance>,
    /// Body-to-appearance bindings resolved through ACT and Design body-map joins.
    pub(crate) bindings: Vec<AppearanceBinding>,
    /// Design material assignments, retained in the native namespace.
    pub(crate) assignments: Vec<DesignMaterialAssignment>,
    /// Per-face appearance assignments awaiting the BREP face-attribute join.
    pub(crate) face_assignments: Vec<FaceAppearanceAssignment>,
    /// Whether the document serializes any body or face appearance assignment.
    ///
    /// Protein assets form a document-local appearance catalog and need not be
    /// assigned to topology. This distinguishes an unassigned catalog from an
    /// assignment that failed to resolve.
    pub(crate) has_topology_assignments: bool,
    /// Distance-valued texture properties omitted because their unit tag has
    /// no defined model-space conversion.
    pub(crate) untyped_distance_properties: usize,
    /// Rejected Protein record diagnostics.
    pub(crate) notes: Vec<String>,
}

/// Decode `.protein` assets and Design and ACT assignments without resolved
/// Design body-map bindings.
///
/// The [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)
/// Design body-map join is skipped. Use [`decode_with_body_bindings`] when the
/// resolved map pairs are available.
pub(crate) fn decode<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<DecodedMaterials, CodecError> {
    decode_with_body_bindings(ctx, scan, &[])
}

fn appearance_equal(
    ctx: &DecodeContext<'_>,
    left: &Appearance,
    right: &Appearance,
) -> Result<bool, CodecError> {
    if !ctx.equal(&left.id, &right.id, "compare F3D appearance IDs")?
        || !ctx.equal(&left.name, &right.name, "compare F3D appearance names")?
        || !ctx.equal(
            &left.asset_guid,
            &right.asset_guid,
            "compare F3D appearance asset GUIDs",
        )?
        || !ctx.equal(
            &left.library_id,
            &right.library_id,
            "compare F3D appearance library IDs",
        )?
        || !ctx.equal(
            &left.visual_guid,
            &right.visual_guid,
            "compare F3D appearance visual GUIDs",
        )?
        || !ctx.equal(
            &left.physical_token,
            &right.physical_token,
            "compare F3D appearance physical tokens",
        )?
        || !ctx.equal(
            &left.schema,
            &right.schema,
            "compare F3D appearance schemas",
        )?
        || !ctx.equal(
            &left.category,
            &right.category,
            "compare F3D appearance categories",
        )?
    {
        return Ok(false);
    }

    match (&left.base_color, &right.base_color) {
        (Some(left), Some(right)) => {
            if !ctx.equal(&left.r(), &right.r(), "compare F3D appearance red values")?
                || !ctx.equal(&left.g(), &right.g(), "compare F3D appearance green values")?
                || !ctx.equal(&left.b(), &right.b(), "compare F3D appearance blue values")?
                || !ctx.equal(&left.a(), &right.a(), "compare F3D appearance alpha values")?
            {
                return Ok(false);
            }
        }
        (None, None) => {}
        _ => return Ok(false),
    }

    if !appearance_properties_equal(ctx, &left.properties, &right.properties)? {
        return Ok(false);
    }
    appearance_textures_equal(ctx, &left.textures, &right.textures)
}

fn appearance_properties_equal(
    ctx: &DecodeContext<'_>,
    left: &std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, FiniteReal>,
    right: &std::collections::BTreeMap<cadmpeg_core::text::NonBlankString, FiniteReal>,
) -> Result<bool, CodecError> {
    if !ctx.equal(
        &left.len(),
        &right.len(),
        "compare F3D appearance property counts",
    )? {
        return Ok(false);
    }
    let left_entries = ctx.admit_iter(left, "compare F3D appearance properties")?;
    let right_entries = ctx.admit_iter(right, "compare F3D appearance properties")?;
    for ((left_name, left_value), (right_name, right_value)) in left_entries.zip(right_entries) {
        if !ctx.equal(
            left_name,
            right_name,
            "compare F3D appearance property names",
        )? || !ctx.equal(
            &left_value.get(),
            &right_value.get(),
            "compare F3D appearance property values",
        )? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn appearance_textures_equal(
    ctx: &DecodeContext<'_>,
    left: &[TextureRef],
    right: &[TextureRef],
) -> Result<bool, CodecError> {
    if !ctx.equal(
        &left.len(),
        &right.len(),
        "compare F3D appearance texture counts",
    )? {
        return Ok(false);
    }
    let left_textures = ctx.admit_iter(left, "compare F3D appearance textures")?;
    let right_textures = ctx.admit_iter(right, "compare F3D appearance textures")?;
    for (left, right) in left_textures.zip(right_textures) {
        if !appearance_texture_equal(ctx, left, right)? {
            return Ok(false);
        }
    }
    Ok(true)
}

fn appearance_texture_equal(
    ctx: &DecodeContext<'_>,
    left: &TextureRef,
    right: &TextureRef,
) -> Result<bool, CodecError> {
    if !ctx.equal(
        &left.asset_guid,
        &right.asset_guid,
        "compare F3D texture asset GUIDs",
    )? || !ctx.equal(&left.slot, &right.slot, "compare F3D texture slots")?
        || !ctx.equal(&left.schema, &right.schema, "compare F3D texture schemas")?
        || !ctx.equal(&left.paths, &right.paths, "compare F3D texture paths")?
        || !ctx.equal(&left.urn, &right.urn, "compare F3D texture URNs")?
        || !appearance_texture_mapping_equal(ctx, &left.mapping, &right.mapping)?
    {
        return Ok(false);
    }
    match (&left.bump, &right.bump) {
        (Some(left), Some(right)) => appearance_bump_equal(ctx, left, right),
        (None, None) => Ok(true),
        _ => Ok(false),
    }
}

fn appearance_texture_mapping_equal(
    ctx: &DecodeContext<'_>,
    left: &TextureMap2d,
    right: &TextureMap2d,
) -> Result<bool, CodecError> {
    if !ctx.equal(
        &left.map_channel,
        &right.map_channel,
        "compare F3D texture map channels",
    )? || !ctx.equal(
        &left.uvw_source,
        &right.uvw_source,
        "compare F3D texture UVW sources",
    )? || !ctx.equal(
        &left.u_offset.get(),
        &right.u_offset.get(),
        "compare F3D texture U offsets",
    )? || !ctx.equal(
        &left.v_offset.get(),
        &right.v_offset.get(),
        "compare F3D texture V offsets",
    )? || !ctx.equal(
        &left.u_scale.get(),
        &right.u_scale.get(),
        "compare F3D texture U scales",
    )? || !ctx.equal(
        &left.v_scale.get(),
        &right.v_scale.get(),
        "compare F3D texture V scales",
    )? || !ctx.equal(
        &left.rotation.get(),
        &right.rotation.get(),
        "compare F3D texture rotations",
    )? || !ctx.equal(
        &left.repeat_u,
        &right.repeat_u,
        "compare F3D texture U repeats",
    )? || !ctx.equal(
        &left.repeat_v,
        &right.repeat_v,
        "compare F3D texture V repeats",
    )? || !ctx.equal(
        &left.real_world_offset_x.get(),
        &right.real_world_offset_x.get(),
        "compare F3D texture real-world X offsets",
    )? || !ctx.equal(
        &left.real_world_offset_y.get(),
        &right.real_world_offset_y.get(),
        "compare F3D texture real-world Y offsets",
    )? || !ctx.equal(
        &left.real_world_scale_x.get(),
        &right.real_world_scale_x.get(),
        "compare F3D texture real-world X scales",
    )? || !ctx.equal(
        &left.real_world_scale_y.get(),
        &right.real_world_scale_y.get(),
        "compare F3D texture real-world Y scales",
    )? {
        return Ok(false);
    }
    Ok(true)
}

fn appearance_bump_equal(
    ctx: &DecodeContext<'_>,
    left: &BumpMap,
    right: &BumpMap,
) -> Result<bool, CodecError> {
    Ok(ctx.equal(
        &left.normal_map,
        &right.normal_map,
        "compare F3D bump normal-map modes",
    )? && ctx.equal(
        &left.depth.get(),
        &right.depth.get(),
        "compare F3D bump depths",
    )? && ctx.equal(
        &left.normal_scale.get(),
        &right.normal_scale.get(),
        "compare F3D bump normal scales",
    )?)
}

/// Decode appearance assets and resolve body bindings through the ordered,
/// blob-qualified Design body-map pairs, closing the design-entity join
/// backbone in [spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials).
pub(crate) fn decode_with_body_bindings<'a>(
    ctx: &DecodeContext<'a>,
    scan: &ContainerScan<'a>,
    body_bindings: &[DesignBodyBinding],
) -> Result<DecodedMaterials, CodecError> {
    let mut out = Vec::new();
    let mut notes = Vec::new();
    let mut untyped_distance_properties = 0usize;
    for entry in ctx.admit_iter(&scan.entries, "scan F3D Protein asset entries")? {
        if !scan.is_design_asset_entry(ctx, entry, ContainerRole::ProteinAssets)? {
            continue;
        }
        let protein = scan.entry_view(ctx, &entry.name)?.ok_or_else(|| {
            CodecError::Malformed("protein archive entry missing from scan".into())
        })?;
        let Some(instance) = instance_properties(ctx, protein)? else {
            continue;
        };
        let record_frames =
            cadmpeg_protein::framing::record_frames_admitted(ctx, instance.window())?;
        let (_catalog_storage, catalog) = definition_catalog(ctx, protein)?;
        let schema_catalog = cadmpeg_protein::SchemaCatalog::load(ctx, protein)?;
        let mut appearances = if let Some(mut schema_catalog) = schema_catalog {
            let outcome = cadmpeg_protein::decode_frames_admitted(
                ctx,
                &mut schema_catalog,
                record_frames.frames(),
            )?;
            for rejected in ctx.admit_iter(&outcome.rejected, "scan rejected Protein records")? {
                let note = ctx.format_retained(
                    format_args!(
                        "Protein {} record {} rejected: {}",
                        entry.name, rejected.ordinal, rejected.detail
                    ),
                    "retain F3D protein rejection note",
                )?;
                ctx.push_vec(&mut notes, note, "collect F3D protein rejection notes")?;
            }
            let records = outcome.records;
            let (mut decoded, untyped_count) = appearances_from_schema_records(ctx, &records)?;
            untyped_distance_properties = untyped_distance_properties
                .checked_add(untyped_count)
                .ok_or_else(|| {
                    ctx.refuse_codec_limit(
                        "count F3D untyped material distances",
                        u64::MAX - 1,
                        u64::MAX,
                    )
                })?;
            let mut decoded_ids_storage =
                ctx.reserve_scoped(0, "index F3D schema appearance IDs")?;
            let decoded_ids = decoded_ids_storage.with_storage(|| {
                ctx.collect_hash_set(
                    decoded.iter().map(|appearance| appearance.id.as_str()),
                    "index F3D schema appearance IDs",
                )
            })?;
            let mut fixed = decode_fixed_logical_records(ctx, record_frames.frames())?;
            ctx.retain_vec(
                &mut fixed,
                |appearance| {
                    Ok(!ctx.contains_hash_set(
                        &decoded_ids,
                        appearance.id.as_str(),
                        "find F3D schema appearance ID",
                    )?)
                },
                "merge F3D fixed appearances",
            )?;
            ctx.append_vec(&mut decoded, &mut { fixed }, "merge F3D fixed appearances")?;
            decoded
        } else {
            decode_fixed_logical_records(ctx, record_frames.frames())?
        };
        for appearance in ctx.admit_iter(&mut appearances, "bind F3D appearance catalog entries")? {
            let Some(name) = appearance.name.as_deref() else {
                continue;
            };
            let Some(schemas) =
                ctx.get_btree_map(&catalog, name, "find F3D appearance catalog asset")?
            else {
                continue;
            };
            // A known schema selects its own entry; otherwise the asset must
            // name exactly one schema.
            let matched = match appearance.schema.as_deref() {
                Some(expected) => ctx.get_key_value_btree_map(
                    schemas,
                    expected,
                    "find F3D appearance catalog schema",
                )?,
                None if schemas.len() == 1 => schemas.first_key_value(),
                None => None,
            };
            if let Some((schema, category)) = matched {
                appearance.schema =
                    Some(ctx.copy_retained_text(schema, "copy F3D catalog schema")?);
                appearance.category = (category.as_deref())
                    .map(|text| ctx.copy_retained_text(text, "copy F3D catalog category"))
                    .transpose()?;
            }
        }
        ctx.append_vec(
            &mut out,
            &mut { appearances },
            "collect F3D asset appearances",
        )?;
    }
    ctx.stable_sort_by(
        &mut out,
        |value| value.id.as_str(),
        Ord::cmp,
        "sort F3D appearance assets",
    )?;
    let window_size = std::num::NonZeroUsize::new(2)
        .ok_or_else(|| CodecError::malformed("appearance window width must be nonzero"))?;
    for pair in ctx
        .admit_iter(&out, "check duplicate F3D appearance assets")?
        .windows(window_size)
    {
        if ctx.equal(&pair[0].id, &pair[1].id, "compare F3D appearance IDs")?
            && !appearance_equal(ctx, &pair[0], &pair[1])?
        {
            return Err(CodecError::malformed(format_args!(
                "F3D appearance asset {} has conflicting payloads",
                pair[0].id
            )));
        }
    }
    ctx.dedup_by(
        &mut out,
        |left, right| ctx.equal(&left.id, &right.id, "compare F3D appearance IDs"),
        "deduplicate F3D appearance assets",
    )?;
    let StreamMaterials {
        assignments,
        overrides: body_overrides,
        face_assignments,
    } = decode_stream_materials(ctx, scan, body_bindings)?;
    let (_act_channels_storage, act_channels) = decode_act_channels(ctx, scan)?;
    let (_object_types_storage, object_types) = decode_design_object_types(ctx, scan)?;
    let mut appearance_index = AppearanceIndex::new(ctx, &out)?;
    for assignment in ctx.admit_iter(&assignments, "match F3D appearance assignments")? {
        if appearance_index
            .for_assignment(ctx, &out, assignment)?
            .is_some()
        {
            continue;
        }
        let appearance = Appearance {
            id: crate::ids::appearance_id(ctx, &assignment.visual_guid)?,
            name: (assignment
                .visual_preset
                .as_ref()
                .map(|field| field.value.as_str()))
            .map(|text| ctx.copy_retained_text(text, "copy F3D assigned appearance name"))
            .transpose()?,
            asset_guid: Some(
                ctx.copy_retained_text(&assignment.visual_guid, "copy F3D assigned asset GUID")?,
            ),
            library_id: None,
            visual_guid: Some(
                ctx.copy_retained_text(&assignment.visual_guid, "copy F3D assigned visual GUID")?,
            ),
            physical_token: (assignment
                .physical_token
                .as_ref()
                .map(|field| field.value.as_str()))
            .map(|text| ctx.copy_retained_text(text, "copy F3D assigned physical token"))
            .transpose()?,
            schema: None,
            category: None,
            base_color: None,
            properties: BTreeMap::new(),
            textures: Vec::new(),
        };
        appearance_index.insert(ctx, out.len(), &appearance)?;
        ctx.push_vec(&mut out, appearance, "collect F3D assignment appearances")?;
    }
    // Each appearance takes the physical token of the first assignment whose
    // visual token matches its own with ASCII case folding.
    let mut first_assignment_storage =
        ctx.reserve_scoped(0, "index F3D assignments by visual token")?;
    let mut first_assignment = BTreeMap::new();
    for assignment in ctx.admit_iter(&assignments, "index F3D assignments by visual token")? {
        first_assignment_storage.with_storage(|| {
            let key = ctx.to_ascii_lowercase(
                &assignment.visual_guid,
                "index F3D assignments by visual token",
            )?;
            if let std::collections::btree_map::Entry::Vacant(entry) = ctx.entry_btree_map(
                &mut first_assignment,
                key,
                "index F3D assignments by visual token",
            )? {
                entry.insert(assignment);
            }
            Ok::<(), CodecError>(())
        })?;
    }
    for appearance in ctx.admit_iter(&mut out, "f3d appearance physical token binding")? {
        let Some(guid) = appearance.visual_guid.as_deref() else {
            continue;
        };
        ctx.charge_work(
            u64_from_index(guid.len()),
            "f3d appearance visual token grammar",
        )?;
        if crate::design::presentation::visual_token(guid).is_none() {
            continue;
        }
        let (key, _key_storage) = ctx
            .with_scoped_storage("f3d visual token assignment search", || {
                ctx.to_ascii_lowercase(guid, "f3d visual token assignment search")
            })?;
        if let Some(assignment) = ctx.get_btree_map(
            &first_assignment,
            key.as_str(),
            "f3d visual token assignment search",
        )? {
            appearance.physical_token = (assignment
                .physical_token
                .as_ref()
                .map(|field| field.value.as_str()))
            .map(|text| ctx.copy_retained_text(text, "copy F3D matched physical token"))
            .transpose()?;
        }
    }
    drop(first_assignment);
    drop(first_assignment_storage);
    let pairs = BodyPairIndex::new(ctx, body_bindings)?;
    let mut bindings = bind_bodies(
        ctx,
        (&out, &appearance_index),
        &assignments,
        &act_channels,
        &object_types,
        &pairs,
    )?;
    // Bodies that already carry an assignment binding keep it.
    let mut bound_storage = ctx.reserve_scoped(0, "index F3D bound appearance bodies")?;
    let mut bound_bodies = std::collections::BTreeSet::new();
    for binding in ctx.admit_iter(&bindings, "index F3D bound appearance bodies")? {
        if let AppearanceTarget::Body(body) = &binding.target {
            bound_storage.with_storage(|| {
                ctx.insert_btree_set(&mut bound_bodies, body, "index F3D bound appearance bodies")
            })?;
        }
    }
    let mut override_bindings = Vec::new();
    for over in ctx.admit_iter(&body_overrides, "match F3D body appearance overrides")? {
        if ctx.contains_btree_set(
            &bound_bodies,
            &over.body,
            "find F3D body appearance binding",
        )? {
            continue;
        }
        let Some(appearance) =
            appearance_index.for_visual_token(ctx, &out, &over.visual_guid, None)?
        else {
            continue;
        };
        ctx.push_vec(
            &mut override_bindings,
            AppearanceBinding {
                id: crate::ids::body_appearance_binding_id(
                    ctx,
                    over.entity_suffix,
                    &over.visual_guid,
                )?,
                target: AppearanceTarget::Body(
                    over.body
                        .try_clone_for_decode(ctx, "copy F3D material body ID")?,
                ),
                appearance: appearance
                    .id
                    .try_clone_for_decode(ctx, "copy F3D material appearance ID")?,
                source_entity_id: None,
                object_type: ctx
                    .get_hash_map(
                        &object_types,
                        &over.entity_suffix,
                        "find F3D override object type",
                    )?
                    .map(|text| ctx.copy_retained_text(text, "copy F3D override object type"))
                    .transpose()?,
                visible: None,
                channels: named_act_channels(
                    ctx,
                    format_args!(
                        "f3d:appearance:body#{}:{}",
                        over.entity_suffix, over.visual_guid
                    ),
                    ctx.get_hash_map(
                        &act_channels,
                        &over.entity_suffix,
                        "find F3D override ACT channels",
                    )?,
                )?,
            },
            "collect F3D override appearance bindings",
        )?;
        bound_storage.with_storage(|| {
            ctx.insert_btree_set(
                &mut bound_bodies,
                &over.body,
                "index F3D bound appearance bodies",
            )
        })?;
    }
    drop(bound_bodies);
    drop(bound_storage);
    ctx.append_vec(
        &mut bindings,
        &mut override_bindings,
        "collect F3D override appearance bindings",
    )?;
    drop(appearance_index);
    let has_topology_assignments =
        !assignments.is_empty() || !body_overrides.is_empty() || !face_assignments.is_empty();
    Ok(DecodedMaterials {
        appearances: out,
        bindings,
        assignments,
        face_assignments,
        has_topology_assignments,
        untyped_distance_properties,
        notes,
    })
}

fn appearances_from_schema_records(
    ctx: &DecodeContext<'_>,
    records: &[cadmpeg_protein::DecodedRecord],
) -> Result<(Vec<Appearance>, usize), CodecError> {
    let mut textures_storage = ctx.reserve_scoped(0, "index F3D texture assets")?;
    let mut textures = BTreeMap::new();
    let mut untyped_distance_properties = 0usize;
    textures_storage.with_storage(|| {
        for record in ctx.admit_iter(records, "scan F3D schema appearance records")? {
            let texture = match texture_asset(ctx, record)? {
                TextureAssetResult::NotTexture => continue,
                TextureAssetResult::UnknownDistanceUnit { count } => {
                    untyped_distance_properties = untyped_distance_properties
                        .checked_add(count)
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit(
                                "count F3D untyped material distances",
                                u64::MAX - 1,
                                u64::MAX,
                            )
                        })?;
                    continue;
                }
                TextureAssetResult::Usable(texture) => texture,
            };
            match ctx.get_btree_map(&textures, &texture.asset_guid, "find F3D texture asset")? {
                None => {
                    let key =
                        ctx.copy_retained_text(&texture.asset_guid, "copy F3D texture asset key")?;
                    ctx.insert_btree_map(&mut textures, key, texture, "index F3D texture assets")?;
                }
                Some(existing) => {
                    if !ctx.equal(existing, &texture, "compare F3D texture asset payloads")? {
                        return Err(CodecError::malformed(format_args!(
                            "Protein texture asset {} has conflicting payloads",
                            texture.asset_guid
                        )));
                    }
                }
            }
        }
        Ok::<(), CodecError>(())
    })?;
    let mut appearances = Vec::new();
    for record in ctx.admit_iter(records, "scan F3D appearance property records")? {
        if matches!(
            record.schema.as_str(),
            "UnifiedBitmapSchema" | "BumpMapSchema"
        ) {
            continue;
        }
        let mut properties = BTreeMap::new();
        let mut connected = Vec::new();
        for (id, property) in
            ctx.admit_iter(&record.properties, "scan F3D appearance properties")?
        {
            if let Some(cadmpeg_protein::property::PropertyValue::Float(value)) = property.value() {
                ctx.insert_btree_map(
                    &mut properties,
                    ctx.copy_retained_text(
                        neutral_property_name(id),
                        "copy F3D appearance property name",
                    )?,
                    *value,
                    "collect F3D appearance properties",
                )?;
            }
            for guid in ctx.admit_iter(property.connections(), "scan F3D texture connections")? {
                if let Some(texture) =
                    ctx.get_btree_map(&textures, guid, "find F3D texture connection")?
                {
                    let reference = texture.to_ref(ctx, id)?;
                    ctx.push_vec(&mut connected, reference, "collect F3D connected textures")?;
                }
            }
        }
        ctx.stable_sort_by(
            &mut connected,
            |value| &value.asset_guid,
            Ord::cmp,
            "sort F3D connected textures",
        )?;
        ctx.stable_sort_by(
            &mut connected,
            |value| &value.slot,
            Ord::cmp,
            "sort F3D connected textures",
        )?;
        let base_color = appearance_base_color(ctx, record)?;
        let appearance = Appearance {
            id: crate::ids::appearance_id(ctx, &record.guid)?,
            name: Some(ctx.copy_retained_text(&record.base, "copy F3D appearance name")?),
            asset_guid: Some(ctx.copy_retained_text(&record.guid, "copy F3D appearance GUID")?),
            library_id: library_id(ctx, &record.asset_lib_id)?,
            visual_guid: if is_physical_schema(&record.schema) {
                None
            } else {
                Some(ctx.copy_retained_text(&record.guid, "copy F3D appearance visual GUID")?)
            },
            physical_token: None,
            schema: Some(ctx.copy_retained_text(&record.schema, "copy F3D appearance schema")?),
            category: None,
            base_color,
            properties: cadmpeg_core::text::named_entries_for_decode(
                ctx,
                format_args!("f3d:design:appearance#{}", record.guid),
                properties,
            )?,
            textures: connected,
        };
        ctx.push_vec(&mut appearances, appearance, "collect F3D appearances")?;
    }
    Ok((appearances, untyped_distance_properties))
}

/// Resolve the one schema member that supplies an appearance's neutral base
/// colour. An enabled common tint replaces the shader family's primary colour;
/// a disabled or absent tint does not participate in selection.
fn appearance_base_color(
    ctx: &DecodeContext<'_>,
    record: &cadmpeg_protein::DecodedRecord,
) -> Result<Option<Color>, CodecError> {
    let Some(property_id) = appearance_base_color_property_id(
        record.schema.as_str(),
        || {
            Ok::<_, CodecError>(matches!(
                ctx.get_btree_map(
                    &record.properties,
                    "common_Tint_toggle",
                    "find F3D common tint toggle",
                )?
                .and_then(|property| property.value()),
                Some(cadmpeg_protein::property::PropertyValue::Boolean(true))
            ))
        },
        || {
            ctx.contains_key_btree_map(
                &record.properties,
                "surface_albedo",
                "check F3D common surface albedo",
            )
        },
    )?
    else {
        return Ok(None);
    };
    color_property(ctx, record, property_id)
}

/// Select the serialized color carrier that represents the neutral base color.
fn appearance_base_color_property_id<E>(
    schema: &str,
    tint_enabled: impl FnOnce() -> Result<bool, E>,
    has_surface_albedo: impl FnOnce() -> Result<bool, E>,
) -> Result<Option<&'static str>, E> {
    if tint_enabled()? {
        return Ok(Some("common_Tint_color"));
    }
    Ok(Some(match schema {
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
        _ if has_surface_albedo()? => "surface_albedo",
        _ => return Ok(None),
    }))
}

fn color_property(
    ctx: &DecodeContext<'_>,
    record: &cadmpeg_protein::DecodedRecord,
    id: &str,
) -> Result<Option<Color>, CodecError> {
    let Some(value) = ctx
        .get_btree_map(&record.properties, id, "find F3D appearance color property")?
        .and_then(|property| property.value())
    else {
        return Ok(None);
    };
    let cadmpeg_protein::property::PropertyValue::Color([r, g, b, a]) = value else {
        return Ok(None);
    };
    Ok(decoded_color([r.get(), g.get(), b.get(), a.get()]))
}

fn decoded_color(values: [f64; 4]) -> Option<Color> {
    // Four components: a fixed-size check needs no admission.
    values
        .iter()
        .all(|value| value.is_finite() && (0.0..=1.0).contains(value))
        .then(|| {
            Color::new(
                f32_from_f64(values[0])?,
                f32_from_f64(values[1])?,
                f32_from_f64(values[2])?,
                f32_from_f64(values[3])?,
            )
        })
        .flatten()
}

/// One per-body appearance override joined through its exact Design body-map pair.
struct BodyAppearanceOverride {
    /// Solved body selected by the exact blob-qualified body-map pair.
    body: BodyId,
    /// The body's design-entity suffix.
    entity_suffix: u64,
    /// Complete serialized visual token bound by the body record.
    visual_guid: DesignVisualToken,
}

impl cadmpeg_core::decode::cost::DecodeCost for BodyAppearanceOverride {
    fn decode_cost(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        operation: &'static str,
    ) -> Result<u64, cadmpeg_core::CodecError> {
        cadmpeg_core::decode::cost::DecodeCost::decode_cost(
            &(&self.body, self.entity_suffix, &self.visual_guid),
            ctx,
            operation,
        )
    }
}

/// One per-face appearance assignment from a Design `BulkStream`.
///
/// The face GUID joins the BREP face that carries the same GUID in its
/// `NEUTRON_Material_attrib_def` attribute
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FaceAppearanceAssignment {
    /// The face GUID shared with the BREP face attribute.
    pub(crate) face_guid: String,
    /// Complete serialized visual token bound by the face record.
    pub(crate) visual_guid: DesignVisualToken,
    /// Face-local neutral color carried by a legacy assignment entry.
    pub(crate) color: Option<Color>,
}

/// Material records read from every Design `BulkStream`.
pub(crate) struct StreamMaterials {
    /// Named body presentations joined to their exact body-map pair.
    pub(crate) assignments: Vec<DesignMaterialAssignment>,
    /// Browser and bare-owner body appearances joined to a solved body.
    overrides: Vec<BodyAppearanceOverride>,
    /// Per-face assignments awaiting the BREP face-attribute join.
    face_assignments: Vec<FaceAppearanceAssignment>,
}

/// Read every material record a Design `BulkStream` carries in one pass over
/// its metadata, body map and body presentations.
///
/// Named presentations become assignments; bare-owner presentations with a
/// browser node and browser-body appearance strings become overrides, joined
/// through the exact BREP body-map pair
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata));
/// face assignments stay inside exact primary-index frames
/// ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#32-materials)).
pub(crate) fn decode_stream_materials(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    body_bindings: &[DesignBodyBinding],
) -> Result<StreamMaterials, CodecError> {
    let pairs = BodyPairIndex::new(ctx, body_bindings)?;
    let mut assignments = Vec::new();
    let mut overrides = Vec::new();
    let mut face_assignments = Vec::new();
    for entry in ctx.admit_iter(&scan.entries, "scan F3D Design stream entries")? {
        if !scan.is_design_stream(ctx, entry, ContainerRole::Bulkstream)? {
            continue;
        }
        let bytes = scan.entry_bytes(ctx, &entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(ctx, scan, &entry.name)?
        else {
            continue;
        };
        let (body_map, _body_map_storage) =
            crate::design::decode::body::body_bindings(ctx, bytes, &metadata)?;
        // A body-map pair is selected by its entity suffix; a repeated suffix
        // is kept as ambiguous and refused when a record names it.
        let (map_by_suffix, _map_by_suffix_storage) = ctx.unique_index(
            body_map
                .iter()
                .map(|binding| (binding.entity_suffix, binding)),
            "index F3D body-map pairs by entity",
        )?;
        let mut browser = browser_body_appearances(ctx, bytes)?;
        for presentation in ctx.admit_iter(
            crate::design::decode::presentation::body_presentations(ctx, bytes, &metadata)?,
            "scan F3D body presentations",
        )? {
            let Some(material) = presentation.material else {
                continue;
            };
            match presentation.owner {
                crate::design::decode::presentation::BodyPresentationOwner::Named {
                    entity_id,
                    entity_id_offset,
                } => {
                    let Some(body_binding) = unique_body_map_pair(
                        ctx,
                        &map_by_suffix,
                        presentation.entity_suffix,
                        "material assignment",
                    )?
                    else {
                        continue;
                    };
                    let assignment = DesignMaterialAssignment {
                        id: crate::ids::native_scoped_id(
                            ctx,
                            &entry.name,
                            "material-assignment",
                            presentation.byte_offset,
                        )?,
                        asm_body_key: body_binding.asm_key,
                        asm_body_key_offset: u64_from_index(body_binding.asm_key_offset),
                        entity_suffix_offset: u64_from_index(body_binding.entity_suffix_offset()),
                        entity_id,
                        entity_id_offset,
                        visual_guid: material.visual_guid,
                        visual_guid_offset: material.visual_guid_offset,
                        physical_token: Some(crate::records::identity::RecordedValue {
                            value: material.physical_token,
                            offset: material.physical_token_offset,
                        }),
                        visual_preset: material.visual_preset.map(|field| {
                            crate::records::identity::RecordedValue {
                                value: field.value,
                                offset: field.offset,
                            }
                        }),
                    };
                    ctx.push_vec(
                        &mut assignments,
                        assignment,
                        "collect F3D material assignments",
                    )?;
                }
                crate::design::decode::presentation::BodyPresentationOwner::Bare
                    if presentation.browser_node.is_some() =>
                {
                    ctx.push_vec(
                        &mut browser,
                        (presentation.entity_suffix, material.visual_guid),
                        "collect F3D browser body appearances",
                    )?;
                }
                crate::design::decode::presentation::BodyPresentationOwner::Bare => {}
            }
        }
        for (entity_suffix, visual_guid) in
            ctx.admit_iter(browser, "join F3D browser body appearances")?
        {
            let Some(map_pair) = unique_body_map_pair(
                ctx,
                &map_by_suffix,
                entity_suffix,
                "browser body appearance",
            )?
            else {
                continue;
            };
            let binding_id = crate::ids::native_design_body_binding_id(
                ctx,
                &entry.name,
                map_pair.asm_key_offset,
            )?;
            let Some(body) = pairs.body(
                ctx,
                &binding_id,
                map_pair.asm_key,
                u64_from_index(map_pair.asm_key_offset),
                map_pair.entity_suffix,
                u64_from_index(map_pair.entity_suffix_offset()),
            )?
            else {
                continue;
            };
            ctx.push_vec(
                &mut overrides,
                BodyAppearanceOverride {
                    body,
                    entity_suffix,
                    visual_guid,
                },
                "collect F3D body appearance overrides",
            )?;
        }
        let frames = crate::metastream::primary_record_frames(ctx, &metadata, bytes.len())?;
        for frame in ctx.admit_iter(&frames, "scan F3D face appearance frames")? {
            ctx.append_vec(
                &mut face_assignments,
                &mut { face_appearance_assignments_in_frame(ctx, &bytes[frame.start..frame.end])? },
                "collect F3D face appearance assignments",
            )?;
        }
    }
    ctx.stable_sort_by(
        &mut overrides,
        |value| value,
        |left, right| {
            left.body
                .cmp(&right.body)
                .then_with(|| left.entity_suffix.cmp(&right.entity_suffix))
                .then_with(|| left.visual_guid.cmp(&right.visual_guid))
        },
        "sort F3D body appearance overrides",
    )?;
    ctx.dedup_by(
        &mut overrides,
        |left, right| {
            Ok(ctx.equal(
                &left.body,
                &right.body,
                "compare F3D body override body IDs",
            )? && ctx.equal(
                &left.entity_suffix,
                &right.entity_suffix,
                "compare F3D body override entity suffixes",
            )? && ctx.eq_ignore_ascii_case(
                &left.visual_guid,
                &right.visual_guid,
                "compare F3D body override visual GUIDs",
            )?)
        },
        "deduplicate F3D body appearance overrides",
    )?;
    Ok(StreamMaterials {
        assignments,
        overrides,
        face_assignments,
    })
}

/// Decode a synthetic test slice as one Design primary-index frame.
#[cfg(test)]
fn face_appearance_assignments(bytes: &[u8]) -> Vec<FaceAppearanceAssignment> {
    crate::test_support::with_decode_context(|ctx| {
        face_appearance_assignments_in_frame(ctx, bytes).expect("face assignment budget")
    })
}

/// Decode face assignments from one exact Design primary-index frame.
fn face_appearance_assignments_in_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<FaceAppearanceAssignment>, CodecError> {
    let strings = lp_utf16_strings(ctx, bytes)?;
    let mut out = legacy_face_appearance_assignments(ctx, bytes, &strings)?;
    ctx.append_vec(
        &mut out,
        &mut { modern_face_appearance_assignments(ctx, bytes, &strings)? },
        "merge F3D modern face appearances",
    )?;
    Ok(out)
}

/// Decode the variable-width legacy face-assignment envelope.
///
/// Every accepted member is adjacent to the next one. This excludes other
/// body-presentation records that share the appearance-library marker.
fn legacy_face_appearance_assignments(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    strings: &[(usize, String)],
) -> Result<Vec<FaceAppearanceAssignment>, CodecError> {
    const LP_GUID_BYTES: usize = 4 + GUID_LEN * 2;
    const COLOR_BYTES: usize = 4 * size_of::<f32>();
    const CARRIER_BYTES: usize = 12;

    let mut out = Vec::new();
    for (index, (marker_at, marker)) in ctx
        .admit_iter(strings, "scan F3D legacy face appearance strings")?
        .enumerate()
    {
        if !ctx.equal(
            marker.as_str(),
            APPEARANCE_LIBRARY_ID,
            "match F3D legacy face appearance marker",
        )? {
            continue;
        }
        let Some((visual_at, visual)) = index.checked_sub(1).and_then(|at| strings.get(at)) else {
            continue;
        };
        let Some((_, visual_len)) = lp_utf16_string_at(ctx, bytes, *visual_at)? else {
            continue;
        };
        if visual_at.checked_add(visual_len) != Some(*marker_at) {
            continue;
        }

        let visual_text = ctx.copy_retained_text(visual, "copy F3D face visual token")?;
        let Some(visual) = DesignVisualToken::new(ctx, visual_text)? else {
            continue;
        };
        let Some(face_at) = visual_at.checked_sub(LP_GUID_BYTES + COLOR_BYTES + CARRIER_BYTES)
        else {
            continue;
        };
        let Some((face_guid, face_len)) = lp_utf16_string_at(ctx, bytes, face_at)? else {
            continue;
        };
        if face_len != LP_GUID_BYTES || !is_lowercase_guid(ctx, &face_guid)? {
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

        let Some((_, marker_len)) = lp_utf16_string_at(ctx, bytes, *marker_at)? else {
            continue;
        };
        let mut cursor = marker_at + marker_len;
        let Some(optional_name_count) = View::u32_le_at(bytes, cursor) else {
            continue;
        };
        if optional_name_count == 0 {
            cursor += 4;
        } else {
            let Some((display_name, display_name_end)) =
                lp_utf16_bounded_charged(ctx, bytes, cursor, 1..=256, "retain F3D UTF-16 string")?
            else {
                continue;
            };
            if ctx
                .admit_iter(display_name.as_str(), "validate F3D face display name")?
                .any(char::is_control)
            {
                continue;
            }
            cursor = display_name_end;
        }
        let Some((selector, selector_len)) = lp_utf16_string_at(ctx, bytes, cursor)? else {
            continue;
        };
        if !legacy_face_selector_is_valid(ctx, selector_kind, &selector)? {
            continue;
        }
        cursor += selector_len;
        if bytes.get(cursor..cursor + 4) != Some(&0_f32.to_le_bytes())
            || bytes.get(cursor + 4..cursor + 8) != Some(&1_f32.to_le_bytes())
        {
            continue;
        }

        ctx.push_vec(
            &mut out,
            FaceAppearanceAssignment {
                face_guid,
                visual_guid: visual,
                color: Some(color),
            },
            "collect F3D legacy face appearances",
        )?;
    }
    Ok(out)
}

/// Decode the normalized RGBA carrier of a legacy face assignment.
fn normalized_legacy_face_color(bytes: &[u8], offset: usize) -> Option<Color> {
    let raw = bytes.get(offset..offset + 4 * size_of::<f32>())?;
    let component = |at: usize| View::f32_le_at(raw, at);
    let color = Color::new(component(0)?, component(4)?, component(8)?, component(12)?)?;
    (color.a() == 1.0).then_some(color)
}

/// Decode the selector-name form flag in the legacy twelve-byte carrier.
fn legacy_face_selector_kind(carrier: Option<&[u8]>) -> Option<u8> {
    let carrier = carrier?;
    (carrier.len() == 12
        && carrier.get(0..2) == Some(&[1, 1])
        && carrier.get(2..11) == Some(&[0; 9])
        && matches!(carrier[11], 0 | 1))
    .then(|| carrier[11])
}

/// Validate the selector family selected by the legacy carrier flag.
fn legacy_face_selector_is_valid(
    ctx: &DecodeContext<'_>,
    kind: u8,
    selector: &str,
) -> Result<bool, CodecError> {
    let (prefix, digit_only) = match kind {
        0 => ("Prism-", true),
        1 => ("Prism", false),
        _ => return Ok(false),
    };
    let Some(suffix) = ctx.strip_prefix(selector, prefix, "select F3D legacy face preset")? else {
        return Ok(false);
    };
    if suffix.is_empty() {
        return Ok(false);
    }
    Ok(ctx
        .admit_iter(suffix, "validate F3D legacy face preset")?
        .all(|character| {
            if digit_only {
                character.is_ascii_digit()
            } else {
                character.is_ascii_alphanumeric()
            }
        }))
}

/// Decode a face-scoped appearance assignment from the paired-library marker
/// form.
///
/// The assignment envelope ends with the visual token and the two library
/// marker GUIDs. Two lower-case GUIDs precede the tail through fixed carrier
/// gaps; the second is the B-rep face-attribute identity. Other paired-library
/// envelopes do not satisfy this grammar.
fn modern_face_appearance_assignments(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    strings: &[(usize, String)],
) -> Result<Vec<FaceAppearanceAssignment>, CodecError> {
    const LP_GUID_BYTES: usize = 4 + GUID_LEN * 2;
    const FIRST_GUID_GAP: usize = 25;
    const FACE_GUID_GAP: usize = 28;

    let mut out = Vec::new();
    for (index, (marker_at, marker)) in ctx
        .admit_iter(strings, "scan F3D modern face appearance strings")?
        .enumerate()
    {
        if !ctx.equal(
            marker.as_str(),
            APPEARANCE_LIBRARY_ID_PAIR[0],
            "match F3D first paired appearance marker",
        )? {
            continue;
        }
        let Some((_, next)) = strings.get(index + 1) else {
            continue;
        };
        if !ctx.equal(
            next.as_str(),
            APPEARANCE_LIBRARY_ID_PAIR[1],
            "match F3D second paired appearance marker",
        )? {
            continue;
        }
        let Some((visual_at, visual)) = index.checked_sub(1).and_then(|at| strings.get(at)) else {
            continue;
        };
        let Some((_, visual_len)) = lp_utf16_string_at(ctx, bytes, *visual_at)? else {
            continue;
        };
        if visual_at.checked_add(visual_len) != Some(*marker_at) {
            continue;
        }

        let visual_text = ctx.copy_retained_text(visual, "copy F3D face visual token")?;
        let Some(visual) = DesignVisualToken::new(ctx, visual_text)? else {
            continue;
        };
        let Some((_, first_library_len)) = lp_utf16_string_at(ctx, bytes, *marker_at)? else {
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
        let Some((face_guid, face_len)) = lp_utf16_string_at(ctx, bytes, face_at)? else {
            continue;
        };
        if face_at.checked_add(face_len) != Some(face_end)
            || !is_lowercase_guid(ctx, &face_guid)?
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
        let Some((first_guid, first_guid_len)) = lp_utf16_string_at(ctx, bytes, first_guid_at)?
        else {
            continue;
        };
        if first_guid_at.checked_add(first_guid_len) != Some(first_guid_end)
            || !is_lowercase_guid(ctx, &first_guid)?
            || !is_first_guid_to_face_gap(bytes.get(first_guid_end..face_at))
        {
            continue;
        }
        ctx.push_vec(
            &mut out,
            FaceAppearanceAssignment {
                face_guid,
                visual_guid: visual,
                color: None,
            },
            "collect F3D modern face appearances",
        )?;
    }
    Ok(out)
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
fn is_lowercase_guid(ctx: &DecodeContext<'_>, value: &str) -> Result<bool, CodecError> {
    if value.len() != GUID_LEN || !is_guid_prefix(value) {
        return Ok(false);
    }
    Ok(ctx
        .admit_iter(&value.as_bytes()[..GUID_LEN], "validate F3D lowercase GUID")?
        .all(|byte| !byte.is_ascii_uppercase()))
}

/// Decode legacy body-presentation records that identify their body through a
/// browser-node GUID rather than a typed body owner.
///
/// The terminating visual marker is shared with face-presentation records.
/// A record is body-owned only when exactly one GUID in its bounded prefix
/// resolves through a browser-node record to one Design entity suffix.
fn browser_body_appearances(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<(u64, DesignVisualToken)>, CodecError> {
    let (nodes, _nodes_storage) =
        crate::design::decode::body::scanned_browser_node_entities(ctx, bytes)?;
    let strings = lp_utf16_strings(ctx, bytes)?;
    let mut out = Vec::new();
    for (index, (_, marker)) in ctx
        .admit_iter(&strings, "scan F3D browser appearance strings")?
        .enumerate()
    {
        // The visual token is the string before the marker, so a marker at the
        // first string names none. The token's index is the proof the
        // candidate window needs, so it is carried rather than derived again.
        let Some(visual_index) = index.checked_sub(1) else {
            continue;
        };
        if !ctx.equal(
            marker.as_str(),
            APPEARANCE_LIBRARY_ID,
            "match F3D browser appearance marker",
        )? {
            continue;
        }
        let visual = &strings[visual_index].1;
        let visual_text = ctx.copy_retained_text(visual, "copy F3D browser visual token")?;
        let Some(visual) = DesignVisualToken::new(ctx, visual_text)? else {
            continue;
        };
        if let Some(entity_suffix) = body_node_candidate(ctx, &strings, visual_index, &nodes)? {
            ctx.push_vec(
                &mut out,
                (entity_suffix, visual),
                "collect F3D browser body appearances",
            )?;
        }
    }
    let mut keep_storage = ctx.reserve_scoped(0, "mark F3D unique browser appearances")?;
    let mut keep = keep_storage.with_storage(|| {
        ctx.alloc_filled(out.len(), false, "mark F3D unique browser appearances")
    })?;
    let mut seen_storage = ctx.reserve_scoped(0, "index F3D unique browser appearances")?;
    let mut seen = std::collections::HashSet::new();
    seen_storage.with_storage(|| {
        for (index, (entity_suffix, visual)) in ctx
            .admit_iter(&out, "select unique F3D browser appearances")?
            .enumerate()
        {
            let text: &str = visual;
            if ctx.contains_hash_set(
                &seen,
                &(*entity_suffix, text),
                "find duplicate F3D browser appearance",
            )? {
                continue;
            }

            ctx.insert_hash_set(
                &mut seen,
                (*entity_suffix, text),
                "index F3D unique browser appearances",
            )?;
            keep[index] = true;
        }
        Ok::<(), CodecError>(())
    })?;
    drop(seen);
    drop(seen_storage);
    let mut index = 0;
    ctx.retain_vec(
        &mut out,
        |_| {
            let retain = keep[index];
            index += 1;
            Ok(retain)
        },
        "select unique F3D browser appearances",
    )?;
    Ok(out)
}

/// The browser-node entity the strings before one visual token name.
///
/// `visual_index` is the index of the visual token itself, which
/// `browser_body_appearances` proved is the string before the marker. The
/// candidate window is the strings between the appearance marker and that
/// token, so the token's own index bounds it and no index is derived twice.
fn body_node_candidate(
    ctx: &DecodeContext<'_>,
    strings: &[(usize, String)],
    visual_index: usize,
    nodes: &std::collections::HashMap<String, u64>,
) -> Result<Option<u64>, CodecError> {
    const APPEARANCE_MARKER: &str = "C1EEA57C-3F56-45FC-B8CB-A9EC46A9994C";
    let mut marker = None;
    for (index, (_, value)) in ctx
        .admit_iter(
            &strings[..=visual_index],
            "find F3D browser appearance boundary",
        )?
        .enumerate()
        .rev()
    {
        if ctx.equal(
            value.as_str(),
            APPEARANCE_MARKER,
            "compare F3D browser appearance marker",
        )? {
            marker = Some(index);
            break;
        }
    }
    let Some(marker) = marker else {
        return Ok(None);
    };
    // Include the three strings before the marker and every string after it.
    let mut first = None;
    for (ordinal, (_, candidate)) in ctx
        .admit_iter(&strings[..visual_index], "scan F3D browser node candidates")?
        .enumerate()
    {
        if ordinal < marker && ordinal.abs_diff(marker) > 3 {
            continue;
        }
        let mut folded_storage = ctx.reserve_scoped(0, "fold F3D browser node candidate")?;
        let entity = folded_storage.with_storage(|| {
            let folded = ctx.to_ascii_lowercase(candidate, "fold F3D browser node candidate")?;
            Ok::<Option<u64>, CodecError>(
                ctx.get_hash_map(nodes, &folded, "find F3D browser node")?
                    .copied(),
            )
        })?;
        let Some(entity) = entity else {
            continue;
        };
        if first.is_some_and(|previous| previous != entity) {
            return Ok(None);
        }
        first = Some(entity);
    }
    Ok(first)
}

fn bind_bodies(
    ctx: &DecodeContext<'_>,
    appearances: (&[Appearance], &AppearanceIndex<'_>),
    assignments: &[DesignMaterialAssignment],
    act_channels: &ActChannelIndex,
    object_types: &std::collections::HashMap<u64, String>,
    pairs: &BodyPairIndex<'_, '_>,
) -> Result<Vec<AppearanceBinding>, CodecError> {
    let (appearances, appearance_index) = appearances;
    let mut out = Vec::new();
    for assignment in ctx.admit_iter(assignments, "scan F3D material body assignments")? {
        let Some(body) = pairs.body(
            ctx,
            &assignment.id,
            assignment.asm_body_key,
            assignment.asm_body_key_offset,
            assignment.entity_id.suffix(),
            assignment.entity_suffix_offset,
        )?
        else {
            continue;
        };
        let Some(appearance) = appearance_index.for_assignment(ctx, appearances, assignment)?
        else {
            continue;
        };
        ctx.push_vec(
            &mut out,
            AppearanceBinding {
                id: crate::ids::assignment_appearance_binding_id(
                    ctx,
                    assignment.entity_id.as_str(),
                    &assignment.visual_guid,
                )?,
                target: AppearanceTarget::Body(body),
                appearance: appearance
                    .id
                    .try_clone_for_decode(ctx, "copy F3D material appearance ID")?,
                source_entity_id: Some(ctx.copy_retained_text(
                    assignment.entity_id.as_str(),
                    "copy F3D binding entity ID",
                )?),
                object_type: ctx
                    .get_hash_map(
                        object_types,
                        &assignment.entity_id.suffix(),
                        "find F3D binding object type",
                    )?
                    .map(|text| ctx.copy_retained_text(text, "copy F3D binding object type"))
                    .transpose()?,
                visible: None,
                channels: named_act_channels(
                    ctx,
                    format_args!(
                        "f3d:appearance:binding#{}:{}",
                        assignment.entity_id.as_str(),
                        assignment.visual_guid
                    ),
                    ctx.get_hash_map(
                        act_channels,
                        &assignment.entity_id.suffix(),
                        "find F3D binding ACT channels",
                    )?,
                )?,
            },
            "collect F3D material body bindings",
        )?;
    }
    Ok(out)
}

/// Whether one indexed key names a single appearance or several.
#[derive(Clone, Copy)]
enum AppearanceSlot {
    One(usize),
    Many,
}

/// Appearances by complete visual token, compared with ASCII case folding,
/// and by preset name.
///
/// The complete visual token is authoritative. A preset name is a secondary
/// identity only when no appearance carries the token, and a missing preset
/// supplies none. Built once, so a lookup reads one key instead of every
/// appearance.
pub(crate) struct AppearanceIndex<'ctx> {
    tokens: BTreeMap<String, AppearanceSlot>,
    names: BTreeMap<String, AppearanceSlot>,
    storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'ctx> AppearanceIndex<'ctx> {
    /// Index every appearance by its position.
    pub(crate) fn new(
        ctx: &'ctx DecodeContext<'_>,
        appearances: &[Appearance],
    ) -> Result<Self, CodecError> {
        let mut index = Self {
            tokens: BTreeMap::new(),
            names: BTreeMap::new(),
            storage: ctx.reserve_scoped(0, "index F3D appearances")?,
        };
        for (position, appearance) in ctx
            .admit_iter(appearances, "index F3D appearances")?
            .enumerate()
        {
            index.insert(ctx, position, appearance)?;
        }
        Ok(index)
    }

    /// Index the appearance stored at `position`.
    fn insert(
        &mut self,
        ctx: &DecodeContext<'_>,
        position: usize,
        appearance: &Appearance,
    ) -> Result<(), CodecError> {
        let Self {
            tokens,
            names,
            storage,
        } = self;
        storage.with_storage(|| {
            if let Some(token) = appearance.visual_guid.as_deref() {
                ctx.charge_work(
                    u64_from_index(token.len()),
                    "f3d appearance visual token grammar",
                )?;
                if crate::design::presentation::visual_token(token).is_some() {
                    let key = ctx.to_ascii_lowercase(token, "index F3D appearance visual token")?;
                    mark_appearance(ctx, tokens, key, position)?;
                }
            }
            if let Some(name) = appearance.name.as_deref() {
                let key = ctx.copy_retained_text(name, "index F3D appearance preset name")?;
                mark_appearance(ctx, names, key, position)?;
            }
            Ok(())
        })
    }

    /// The unique appearance one Design assignment selects.
    pub(crate) fn for_assignment<'a>(
        &self,
        ctx: &DecodeContext<'_>,
        appearances: &'a [Appearance],
        assignment: &DesignMaterialAssignment,
    ) -> Result<Option<&'a Appearance>, CodecError> {
        self.for_visual_token(
            ctx,
            appearances,
            &assignment.visual_guid,
            assignment
                .visual_preset
                .as_ref()
                .map(|field| field.value.as_str()),
        )
    }

    /// The unique appearance a complete serialized visual token selects,
    /// falling back to `fallback_name` when no appearance carries the token.
    pub(crate) fn for_visual_token<'a>(
        &self,
        ctx: &DecodeContext<'_>,
        appearances: &'a [Appearance],
        token: &DesignVisualToken,
        fallback_name: Option<&str>,
    ) -> Result<Option<&'a Appearance>, CodecError> {
        let (key, _key_storage) = ctx
            .with_scoped_storage("find F3D appearance visual token", || {
                ctx.to_ascii_lowercase(token, "find F3D appearance visual token")
            })?;
        match ctx
            .get_btree_map(
                &self.tokens,
                key.as_str(),
                "find F3D appearance visual token",
            )?
            .copied()
        {
            Some(AppearanceSlot::One(position)) => return Ok(appearances.get(position)),
            Some(AppearanceSlot::Many) => {
                return Err(CodecError::malformed(
                    "F3D visual token matches multiple appearance assets",
                ))
            }
            None => {}
        }
        let Some(name) = fallback_name else {
            return Ok(None);
        };
        match ctx
            .get_btree_map(&self.names, name, "find F3D appearance preset name")?
            .copied()
        {
            Some(AppearanceSlot::One(position)) => Ok(appearances.get(position)),
            Some(AppearanceSlot::Many) => Err(CodecError::malformed(
                "F3D visual preset matches multiple appearance assets",
            )),
            None => Ok(None),
        }
    }
}

/// The appearance one assignment selects, for writer validation, under the
/// writing policy.
pub(crate) fn writer_appearance_for_assignment<'a>(
    appearances: &'a [Appearance],
    assignment: &DesignMaterialAssignment,
) -> Result<Option<&'a Appearance>, CodecError> {
    crate::writer::primitives::with_writing_context(|ctx| {
        AppearanceIndex::new(ctx, appearances)?.for_assignment(ctx, appearances, assignment)
    })
}

fn mark_appearance(
    ctx: &DecodeContext<'_>,
    slots: &mut BTreeMap<String, AppearanceSlot>,
    key: String,
    position: usize,
) -> Result<(), CodecError> {
    match ctx.entry_btree_map(slots, key, "index F3D appearances")? {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(AppearanceSlot::One(position));
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            *entry.get_mut() = AppearanceSlot::Many;
        }
    }
    Ok(())
}

/// The exact identity of one blob-qualified body-map pair: the owner stream,
/// the ASM body key and the entity suffix with their offsets.
type BodyPairKey<'a> = (&'a str, u64, u64, u64, u64);

/// Decoded body-map pairs by their exact identity, built once per decode.
///
/// ASM keys are local to the pair's BREP basename, so the owner stream is
/// part of the key. A repeated identity is kept as ambiguous and refused when
/// an owner selects it.
struct BodyPairIndex<'a, 'ctx> {
    pairs: std::collections::HashMap<Option<BodyPairKey<'a>>, Option<&'a DesignBodyBinding>>,
    _storage: cadmpeg_core::decode::ScopedReservation<'ctx>,
}

impl<'a, 'ctx> BodyPairIndex<'a, 'ctx> {
    fn new(
        ctx: &'ctx DecodeContext<'_>,
        body_bindings: &'a [DesignBodyBinding],
    ) -> Result<Self, CodecError> {
        // Pairs whose id names no stream cannot match an owner; they are kept
        // under the absent key, which no lookup asks for.
        let (pairs, storage) = ctx.unique_index(
            body_bindings.iter().map(|binding| {
                let key = crate::ids::native_stream(binding.id()).map(|stream| {
                    (
                        stream,
                        binding.asm_body_key,
                        binding.asm_body_key_offset(),
                        binding.entity_suffix,
                        binding.entity_suffix_offset(),
                    )
                });
                (key, binding)
            }),
            "index F3D material body-map pairs",
        )?;
        Ok(Self {
            pairs,
            _storage: storage,
        })
    }

    /// The solved body of the one pair `owner_id` names exactly.
    fn body(
        &self,
        ctx: &DecodeContext<'_>,
        owner_id: &str,
        asm_body_key: u64,
        asm_body_key_offset: u64,
        entity_suffix: u64,
        entity_suffix_offset: u64,
    ) -> Result<Option<BodyId>, CodecError> {
        let owner_stream = crate::ids::native_stream(owner_id).ok_or_else(|| {
            CodecError::malformed(format_args!(
                "F3D material owner has no native stream: {owner_id}"
            ))
        })?;
        let key = Some((
            owner_stream,
            asm_body_key,
            asm_body_key_offset,
            entity_suffix,
            entity_suffix_offset,
        ));
        let binding =
            match ctx.get_hash_map(&self.pairs, &key, "find exact F3D material body-map pair")? {
                None => return Ok(None),
                Some(Some(binding)) => *binding,
                Some(None) => {
                    return Err(CodecError::malformed(format_args!(
                        "F3D material owner {owner_id} matches multiple exact body-map pairs"
                    )))
                }
            };
        binding
            .body
            .as_ref()
            .map(|body| body.try_clone_for_decode(ctx, "copy F3D material body ID"))
            .transpose()
    }
}

fn decode_design_object_types<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<
    (
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        std::collections::HashMap<u64, String>,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "index F3D Design object types")?;
    let mut out = std::collections::HashMap::new();
    storage.with_storage(|| {
        for entry in ctx.admit_iter(&scan.entries, "scan F3D Design MetaStreams")? {
            if !scan.is_design_stream(ctx, entry, ContainerRole::Metastream)? {
                continue;
            }
            let bytes = scan.entry_bytes(ctx, &entry.name)?;
            let mut position = 0usize;
            while position + 8 <= bytes.len() {
                ctx.charge_work(1, "scan F3D Design object-type positions")?;
                let Some((object_type, after_type)) = lp_ascii_printable(ctx, bytes, position)?
                else {
                    position += 1;
                    continue;
                };
                if !ctx.all_by(
                    object_type.as_bytes(),
                    |byte| Ok(byte.is_ascii_alphabetic()),
                    "validate F3D Design object type",
                )? {
                    position += 1;
                    continue;
                }
                let mut view = View::over_retained(&bytes[after_type..]);
                let Some(count) = view.u32_le() else {
                    break;
                };
                if count > 200 {
                    position += 1;
                    continue;
                }
                if view.counted(u64::from(count), 8).is_none() {
                    position += 1;
                    continue;
                }
                for _ in ctx.admit_iter(&(0..count), "scan F3D Design object-type ids")? {
                    let Some(id) = view.u64_le() else {
                        break;
                    };
                    let value =
                        ctx.copy_retained_text(object_type, "copy F3D Design object type")?;
                    ctx.insert_hash_map(&mut out, id, value, "index F3D Design object types")?;
                }
                position = after_type + view.position();
            }
        }
        Ok::<(), CodecError>(())
    })?;
    Ok((storage, out))
}

/// ACT channel names and values keyed by entity suffix.
type ActChannelIndex = std::collections::HashMap<u64, BTreeMap<String, String>>;

fn decode_act_channels<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'a>,
    scan: &ContainerScan<'a>,
) -> Result<
    (
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        ActChannelIndex,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "index F3D ACT entities")?;
    let mut out = std::collections::HashMap::new();
    storage.with_storage(|| {
        for entry in ctx.admit_iter(&scan.entries, "scan F3D ACT stream entries")? {
            if !scan.is_act_stream(ctx, entry)? {
                continue;
            }
            let bytes = scan.entry_bytes(ctx, &entry.name)?;
            let mut position = 0usize;
            while position + 4 <= bytes.len() {
                ctx.charge_work(1, "scan F3D ACT stream positions")?;
                let Some((tag, after_tag)) = lp_ascii_printable(ctx, bytes, position)? else {
                    position += 1;
                    continue;
                };
                // A three-byte tag: the digit check is fixed-size.
                if tag.len() != 3 || !tag.bytes().all(|byte| byte.is_ascii_digit()) {
                    position += 1;
                    continue;
                }
                let Some(count) = View::u32_le_at(bytes, after_tag + 14) else {
                    break;
                };
                if bytes.get(after_tag + 4..after_tag + 14) != Some(&[0u8; 10]) {
                    position += 1;
                    continue;
                }
                if !(1..=8).contains(&count) {
                    position += 1;
                    continue;
                }
                let mut cursor = after_tag + 18;
                let mut channels = BTreeMap::new();
                let mut valid = true;
                for _ in ctx.admit_iter(&(0..count), "scan F3D ACT channel-group rows")? {
                    let Some((name, after_name)) = lp_ascii_printable(ctx, bytes, cursor)? else {
                        valid = false;
                        break;
                    };
                    let Some((guid, after_guid)) = lp_utf16_bounded_charged(
                        ctx,
                        bytes,
                        after_name,
                        1..=64,
                        "retain F3D UTF-16 string",
                    )?
                    else {
                        valid = false;
                        break;
                    };
                    if guid.len() != 36 {
                        valid = false;
                        break;
                    }
                    let name = ctx.copy_retained_text(name, "copy F3D ACT channel name")?;
                    ctx.insert_btree_map(&mut channels, name, guid, "collect F3D ACT channels")?;
                    cursor = after_guid;
                }
                if valid {
                    if let Some((entity, end)) = lp_utf16_bounded_charged(
                        ctx,
                        bytes,
                        cursor,
                        1..=64,
                        "retain F3D UTF-16 string",
                    )? {
                        if let Some(suffix) = entity_suffix(ctx, &entity)? {
                            ctx.insert_hash_map(
                                &mut out,
                                suffix,
                                channels,
                                "index F3D ACT entities",
                            )?;
                        }
                        position = end;
                        continue;
                    }
                }
                position += 1;
            }
        }
        Ok::<(), CodecError>(())
    })?;
    Ok((storage, out))
}

fn entity_suffix(ctx: &DecodeContext<'_>, value: &str) -> Result<Option<u64>, CodecError> {
    let Some((_, suffix)) = ctx.split_once(value, "_", "split F3D entity suffix")? else {
        return Ok(None);
    };
    Ok(ctx
        .parse_text::<u64>(suffix, "parse F3D entity suffix")?
        .ok())
}

fn lp_utf16_strings(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<Vec<(usize, String)>, CodecError> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + 4 <= bytes.len() {
        ctx.charge_work(1, "scan F3D UTF-16 string candidates")?;
        let Some(count) =
            View::u32_le_at(bytes, offset).and_then(|count| usize::try_from(count).ok())
        else {
            offset += 1;
            continue;
        };
        let Some(payload_at) = offset.checked_add(4) else {
            offset += 1;
            continue;
        };
        if !(2..=256).contains(&count)
            || !utf16_string_prefix_is_text(ctx, bytes, payload_at, count)?
        {
            offset += 1;
            continue;
        }
        if let Some((value, record_len)) = lp_utf16_string_at(ctx, bytes, offset)? {
            ctx.push_vec(&mut out, (offset, value), "collect F3D UTF-16 strings")?;
            offset += record_len;
        } else {
            offset += 1;
        }
    }
    Ok(out)
}

/// Reject an unframed string candidate before decoding its full declared run.
///
/// Appearance streams contain several unrelated binary records, so scanning
/// every byte can encounter a plausible length word whose payload is not text.
/// Checking only the first four code units keeps the heuristic bounded while
/// accepting supplementary-plane characters whose surrogate pair spans the
/// prefix boundary. The full strict decode remains authoritative.
fn utf16_string_prefix_is_text(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    payload_at: usize,
    count: usize,
) -> Result<bool, CodecError> {
    let prefix_count = count.min(4);
    let Some(prefix_bytes_len) = prefix_count.checked_mul(2) else {
        return Ok(false);
    };
    let Some(prefix_end) = payload_at.checked_add(prefix_bytes_len) else {
        return Ok(false);
    };
    let Some(prefix_bytes) = bytes.get(payload_at..prefix_end) else {
        return Ok(false);
    };
    let prefix = ctx
        .admit_iter(prefix_bytes, "validate F3D UTF-16 string prefix")?
        .enumerate()
        .step_by(2);
    let mut high_surrogate = false;
    for (offset, _) in prefix {
        let Some(unit) = View::u16_le_at(prefix_bytes, offset) else {
            return Ok(false);
        };
        if high_surrogate && !(0xdc00..=0xdfff).contains(&unit) {
            return Ok(false);
        }
        match unit {
            0 => return Ok(false),
            0xd800..=0xdbff => high_surrogate = true,
            0xdc00..=0xdfff if !high_surrogate => return Ok(false),
            0xdc00..=0xdfff => high_surrogate = false,
            value if char::from_u32(u32::from(value)).is_some_and(|value| !value.is_control()) => {
                high_surrogate = false;
            }
            _ => return Ok(false),
        }
    }
    Ok(true)
}

/// Decode one LP-UTF16 string at `offset`. Rejects a count outside 2..=256,
/// invalid UTF-16, or a control character.
fn lp_utf16_string_at(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    offset: usize,
) -> Result<Option<(String, usize)>, CodecError> {
    let Some((value, end)) =
        lp_utf16_bounded_charged(ctx, bytes, offset, 2..=256, "retain F3D UTF-16 string")?
    else {
        return Ok(None);
    };
    if ctx
        .admit_iter(value.as_str(), "check F3D Protein string controls")?
        .any(char::is_control)
    {
        return Ok(None);
    }
    Ok(Some((value, end - offset)))
}

/// Select the sole ordered body-map pair carrying one material owner's Design
/// entity suffix. More than one pair leaves the owner ambiguous.
fn unique_body_map_pair<'a>(
    ctx: &DecodeContext<'_>,
    by_suffix: &std::collections::HashMap<
        u64,
        Option<&'a crate::design::decode::body::BodyBinding>,
    >,
    entity_suffix: u64,
    owner_kind: &str,
) -> Result<Option<&'a crate::design::decode::body::BodyBinding>, CodecError> {
    match ctx.get_hash_map(by_suffix, &entity_suffix, "find F3D body-map pair")? {
        None => Ok(None),
        Some(Some(binding)) => Ok(Some(*binding)),
        Some(None) => Err(CodecError::malformed(format_args!(
            "F3D {owner_kind} entity {entity_suffix} matches multiple body-map pairs"
        ))),
    }
}

/// Open a nested Protein ZIP member through the session archive expander so
/// per-expand and cumulative decompressed ceilings bind.
fn instance_properties<'a>(
    ctx: &DecodeContext<'a>,
    protein: View<'a>,
) -> Result<Option<View<'a>>, CodecError> {
    nested_entry(ctx, protein, "AssetData/InstanceProperties.bin")
}

/// Definition categories by asset identity, then schema.
type DefinitionCatalogIndex = BTreeMap<String, BTreeMap<String, Option<String>>>;

fn definition_catalog<'ctx, 'a>(
    ctx: &'ctx DecodeContext<'a>,
    protein: View<'a>,
) -> Result<
    (
        cadmpeg_core::decode::ScopedReservation<'ctx>,
        DefinitionCatalogIndex,
    ),
    CodecError,
> {
    let mut storage = ctx.reserve_scoped(0, "index F3D definition catalog")?;
    let Some(entry) = nested_entry(ctx, protein, "AssetData/DefinitionIteratorProperties.bin")?
    else {
        return Ok((storage, BTreeMap::new()));
    };
    let frames = cadmpeg_protein::framing::record_frames_admitted(ctx, entry.window())?;
    let mut definitions = BTreeMap::new();
    storage.with_storage(|| {
        for frame in ctx.admit_iter(frames.frames(), "scan F3D definition records")? {
            let definition = decode_definition_catalog_record(ctx, frame.bytes())?;
            merge_definition_catalog_record(ctx, &mut definitions, definition)?;
        }
        Ok::<(), CodecError>(())
    })?;
    Ok((storage, definitions))
}

#[derive(Debug, PartialEq, Eq)]
struct DefinitionCatalog {
    schema: String,
    asset_id: String,
    category: Option<String>,
}

/// Index one definition record by asset and schema. Records that repeat an
/// (asset, schema) pair with a different category leave its category unknown.
fn merge_definition_catalog_record(
    ctx: &DecodeContext<'_>,
    definitions: &mut DefinitionCatalogIndex,
    definition: DefinitionCatalog,
) -> Result<(), CodecError> {
    let DefinitionCatalog {
        schema,
        asset_id,
        category,
    } = definition;
    let schemas =
        match ctx.entry_btree_map(definitions, asset_id, "index F3D definition catalog")? {
            std::collections::btree_map::Entry::Vacant(entry) => entry.insert(BTreeMap::new()),
            std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
        };
    match ctx.entry_btree_map(schemas, schema, "index F3D definition catalog schemas")? {
        std::collections::btree_map::Entry::Vacant(entry) => {
            entry.insert(category);
        }
        std::collections::btree_map::Entry::Occupied(mut entry) => {
            if !ctx.equal(
                entry.get(),
                &category,
                "compare F3D definition catalog categories",
            )? {
                *entry.get_mut() = None;
            }
        }
    }
    Ok(())
}

fn decode_definition_catalog_record(
    ctx: &DecodeContext<'_>,
    record: &[u8],
) -> Result<DefinitionCatalog, CodecError> {
    let malformed = malformed_definition_catalog_record;
    if !record.starts_with(RECORD_MARKER) {
        return Err(malformed("marker", 0));
    }
    let mut position = RECORD_MARKER.len();
    let schema = take_lp_utf8_charged(ctx, record, &mut position)?
        .ok_or_else(|| malformed("schema", position))?;
    let flag = *record
        .get(position)
        .ok_or_else(|| malformed("flag", position))?;
    if flag > 1 {
        return Err(malformed("flag", position));
    }
    position += 1;
    let asset_id = take_lp_utf8_charged(ctx, record, &mut position)?
        .ok_or_else(|| malformed("asset identifier", position))?;
    take_lp_utf8_charged(ctx, record, &mut position)?
        .ok_or_else(|| malformed("base asset identifier", position))?;
    let version =
        View::u32_le_at(record, position).ok_or_else(|| malformed("format version", position))?;
    position += 4;
    if version > 3 {
        return Err(malformed("format version", position - 4));
    }
    let category = if version >= 2 {
        Some(
            take_lp_utf8_charged(ctx, record, &mut position)?
                .ok_or_else(|| malformed("category", position))?,
        )
    } else {
        None
    };
    if version >= 1 {
        take_lp_utf8_charged(ctx, record, &mut position)?
            .ok_or_else(|| malformed("group", position))?;
    }
    if version == 3 {
        take_lp_utf8_charged(ctx, record, &mut position)?
            .ok_or_else(|| malformed("subgroup", position))?;
    }
    take_lp_utf8_charged(ctx, record, &mut position)?
        .ok_or_else(|| malformed("description", position))?;
    consume_catalog_strings(ctx, record, &mut position)?;
    consume_catalog_strings(ctx, record, &mut position)?;
    if ctx
        .admit_iter(&record[position..], "check F3D definition trailing padding")?
        .any(|byte| *byte != 0)
    {
        return Err(malformed("trailing padding", position));
    }
    Ok(DefinitionCatalog {
        schema,
        asset_id,
        category,
    })
}

fn malformed_definition_catalog_record(_field: &str, _position: usize) -> CodecError {
    CodecError::Malformed("Protein definition catalog record is malformed".into())
}

fn consume_catalog_strings(
    ctx: &DecodeContext<'_>,
    record: &[u8],
    position: &mut usize,
) -> Result<(), CodecError> {
    let count = View::u32_le_at(record, *position)
        .ok_or_else(|| malformed_definition_catalog_record("string count", *position))?;
    *position += 4;
    let count = record
        .len()
        .checked_sub(*position)
        .and_then(|left| bounded_len(u64::from(count), 4, left))
        .ok_or_else(|| malformed_definition_catalog_record("string count", *position))?;
    for _ in ctx.admit_iter(&(0..count), "scan F3D catalog strings")? {
        take_lp_utf8_charged(ctx, record, position)?
            .ok_or_else(|| malformed_definition_catalog_record("string", *position))?;
    }
    Ok(())
}

fn nested_entry<'a>(
    ctx: &DecodeContext<'a>,
    protein: View<'a>,
    suffix: &str,
) -> Result<Option<View<'a>>, CodecError> {
    let archive = match ArchiveSnapshot::new(ctx, protein) {
        Ok(archive) => archive,
        Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
        Err(_) => return Ok(None),
    };
    for entry in ctx.admit_iter(archive.entries(), "scan nested F3D Protein entries")? {
        if ctx.ends_with(&entry.name, suffix, "match nested F3D Protein entry suffix")? {
            return Ok(Some(archive.open(ctx, &entry.name)?));
        }
    }
    Ok(None)
}

/// Decode the fixed source-less layouts emitted by [`encode_protein`]. Native
/// Protein assets package schemas and use the schema-driven path instead.
fn decode_fixed_logical_records(
    ctx: &DecodeContext<'_>,
    frames: &[cadmpeg_protein::framing::RecordFrame],
) -> Result<Vec<Appearance>, CodecError> {
    let mut appearances = Vec::new();
    for frame in ctx.admit_iter(frames, "scan F3D fixed appearance records")? {
        if let Some(appearance) = decode_fixed_record(ctx, frame.bytes())? {
            ctx.push_vec(
                &mut appearances,
                appearance,
                "collect F3D fixed appearances",
            )?;
        }
    }
    Ok(appearances)
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
fn decode_fixed_record(
    ctx: &DecodeContext<'_>,
    record: &[u8],
) -> Result<Option<Appearance>, CodecError> {
    let mut position = RECORD_MARKER.len();
    let Some(schema) = take_lp_utf8_charged(ctx, record, &mut position)? else {
        return Ok(None);
    };
    let Some(guid) = take_lp_utf8_charged(ctx, record, &mut position)? else {
        return Ok(None);
    };
    let Some(base) = take_lp_utf8_charged(ctx, record, &mut position)? else {
        return Ok(None);
    };
    let Some(asset_lib_id) = take_lp_utf8_charged(ctx, record, &mut position)? else {
        return Ok(None);
    };
    let color = match schema.as_str() {
        "GenericSchema" => {
            let Some(delta) = generic_connection_delta(record, position) else {
                return Ok(None);
            };
            fixed_rgba(record, position + 112 + delta)
        }
        "PrismOpaqueSchema" | "PrismMetalSchema" => fixed_rgba(record, position + 8),
        "PrismTransparentSchema" => fixed_rgba(record, position + 121),
        "PhysMatSchema"
        | "StructuralMetalSchema"
        | "StructuralPlasticSchema"
        | "ThermalSolidSchema" => None,
        _ => return Ok(None),
    };
    let mut properties = BTreeMap::new();
    if schema == "GenericSchema" {
        let Some(delta) = generic_connection_delta(record, position) else {
            return Ok(None);
        };
        fixed_tagged_scalar(
            ctx,
            &mut properties,
            "reflectivity_at_0deg",
            record,
            position + 171 + delta,
        )?;
        fixed_tagged_scalar(
            ctx,
            &mut properties,
            "refraction_index",
            record,
            position + 197 + delta,
        )?;
    } else if schema == "PrismOpaqueSchema" {
        if let Some(marker) = ctx.find_bytes_from(
            record,
            b"\x0e\x20\x00\x00",
            position,
            "find F3D roughness carrier",
        )? {
            fixed_scalar(
                ctx,
                &mut properties,
                "surface_roughness",
                record,
                marker + 4,
            )?;
        }
    } else if schema == "PrismTransparentSchema" {
        fixed_scalar(
            ctx,
            &mut properties,
            "refraction_index",
            record,
            position + 169,
        )?;
    }
    let properties = cadmpeg_core::text::named_entries_for_decode(
        ctx,
        format_args!("f3d:design:appearance#{guid}"),
        properties,
    )?;
    Ok(Some(Appearance {
        id: crate::ids::appearance_id(ctx, &guid)?,
        name: Some(base),
        asset_guid: Some(ctx.copy_retained_text(&guid, "copy F3D fixed appearance GUID")?),
        library_id: library_id(ctx, &asset_lib_id)?,
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
    }))
}

fn fixed_scalar(
    ctx: &DecodeContext<'_>,
    out: &mut BTreeMap<String, FiniteReal>,
    name: &str,
    bytes: &[u8],
    offset: usize,
) -> Result<(), CodecError> {
    if let Some(value) = View::f64_le_at(bytes, offset).and_then(FiniteReal::new) {
        let name = ctx.copy_retained_text(name, "copy F3D fixed property name")?;
        ctx.insert_btree_map(out, name, value, "collect F3D fixed properties")?;
    }
    Ok(())
}

fn fixed_tagged_scalar(
    ctx: &DecodeContext<'_>,
    out: &mut BTreeMap<String, FiniteReal>,
    name: &str,
    bytes: &[u8],
    offset: usize,
) -> Result<(), CodecError> {
    if bytes.get(offset..offset + 4) == Some(b"\x0c\x00\x00\x00") {
        fixed_scalar(ctx, out, name, bytes, offset + 4)?;
    }
    Ok(())
}

fn fixed_rgba(bytes: &[u8], offset: usize) -> Option<Color> {
    let mut values = [0.0; 4];
    for (ordinal, value) in values.iter_mut().enumerate() {
        *value = View::f64_le_at(bytes, offset + ordinal * 8)?;
    }
    decoded_color(values)
}

/// Measure the `GenericSchema` connection block at `value_block`. The block holds
/// at most eight length-prefixed connections, so the walk is bounded.
fn generic_connection_delta(record: &[u8], value_block: usize) -> Option<usize> {
    let slot = value_block.checked_add(102)?;
    match record.get(slot) {
        Some(0) => Some(0),
        Some(1) if slot.checked_add(6).is_some_and(|end| end <= record.len()) => {
            let count = View::u32_le_at(record, slot + 2).map(index_from_u32)?;
            if count > 8 {
                return None;
            }
            let mut position = slot + 6;
            for _ in 0..count {
                let length = View::u32_le_at(record, position).map(index_from_u32)?;
                let end = position.checked_add(4)?.checked_add(length)?;
                record.get(position..end)?;
                position = end;
            }
            position.checked_sub(slot + 1)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
