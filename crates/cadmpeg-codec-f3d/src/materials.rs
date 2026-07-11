// SPDX-License-Identifier: Apache-2.0
//! Fusion `.protein` appearance asset decoding.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};

use cadmpeg_ir::appearance::Appearance;
use cadmpeg_ir::appearance::{AppearanceBinding, AppearanceTarget};
use cadmpeg_ir::codec::{CodecError, ReadSeek};
use cadmpeg_ir::design::DesignMaterialAssignment;
use cadmpeg_ir::ids::{AppearanceId, BodyId};
use cadmpeg_ir::topology::Color;

use crate::container::{self, role, ContainerScan};

const PAGE_SIZE: usize = 0x88;
const RECORD_MARKER: &[u8] = b"\x80\x00\x01\x00";

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
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            CodecError::Malformed(format!("cannot read nested Protein entry: {error}"))
        })?;
        let name = entry.name().to_owned();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut bytes)?;
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
        let schema = take_lp(record, &mut position).ok_or_else(|| {
            CodecError::Malformed("Protein appearance schema is truncated".into())
        })?;
        let guid = take_lp(record, &mut position)
            .ok_or_else(|| CodecError::Malformed("Protein appearance GUID is truncated".into()))?;
        let _ = take_lp(record, &mut position);
        let _ = take_lp(record, &mut position);
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

/// Decoded appearance assets and body bindings from a single `decode` pass:
/// the merged `.protein`/design/ACT appearance records ([`Appearance`]) and
/// the body-to-appearance bindings resolved through the design-entity join
/// backbone (spec §8.2).
#[derive(Default)]
pub struct DecodedMaterials {
    /// Merged appearance records, deduplicated by [`AppearanceId`].
    pub appearances: Vec<Appearance>,
    /// Body-to-appearance bindings resolved via ACT/design/ASM body-key joins.
    pub bindings: Vec<AppearanceBinding>,
}

/// Decode all `.protein` appearance assets and design/ACT appearance
/// assignments reachable from `scan`, without resolving ASM body-key bindings
/// (spec §8.2's `asm_body_key` join is skipped; callers with body keys should
/// use [`decode_with_bodies`]).
pub fn decode(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<DecodedMaterials, CodecError> {
    decode_with_bodies(reader, scan, &std::collections::HashMap::new())
}

/// Decode appearance assets and bindings, resolving each binding's body
/// through `body_keys` (`BodyId` → `asm_body_key`, the ASM `Body.chunk[1]`
/// value) to close the design-entity join backbone described in spec §8.2.
pub fn decode_with_bodies<S: std::hash::BuildHasher>(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
    body_keys: &std::collections::HashMap<BodyId, u64, S>,
) -> Result<DecodedMaterials, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::PROTEIN)
    {
        let payload = container::decompress_entry(reader, &entry.name)?;
        let Some(instance) = instance_properties(&payload) else {
            continue;
        };
        let Some(logical) = dechunk(&instance) else {
            continue;
        };
        let catalog = definition_catalog(&payload);
        let mut appearances = decode_logical_records(&logical, &entry.name);
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
    let assignments = decode_design_assignments(reader, scan)?;
    let act_channels = decode_act_channels(reader, scan)?;
    let object_types = decode_design_object_types(reader, scan)?;
    for assignment in &assignments {
        if !out.iter().any(|appearance| {
            appearance.visual_guid.as_deref() == Some(&assignment.visual_guid)
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
            });
        }
    }
    for appearance in &mut out {
        if let Some(assignment) = assignments
            .iter()
            .find(|assignment| appearance.visual_guid.as_deref() == Some(&assignment.visual_guid))
        {
            appearance.physical_token = assignment.physical_token.clone();
        }
    }
    let bindings = bind_bodies(&out, &assignments, &act_channels, &object_types, body_keys);
    Ok(DecodedMaterials {
        appearances: out,
        bindings,
    })
}

pub(crate) fn decode_design_assignments(
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<Vec<DesignMaterialAssignment>, CodecError> {
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        let body_map = decode_body_map(&bytes);
        let strings = lp_utf16_strings(&bytes);
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
            if let Some((&asm_body_key, &(_, suffix_offset))) = body_map
                .iter()
                .find(|(_, (suffix, _))| *suffix == entity_suffix)
            {
                out.push(DesignMaterialAssignment {
                    id: format!("f3d:{}:material-assignment#{entity_offset}", entry.name),
                    asm_body_key,
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
                appearance.visual_guid.as_deref() == Some(&assignment.visual_guid)
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
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<std::collections::HashMap<u64, String>, CodecError> {
    let mut out = std::collections::HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::METASTREAM && entry.name.contains("Design"))
    {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        let mut position = 0usize;
        while position + 8 <= bytes.len() {
            let Some((object_type, after_type)) = lp_ascii(&bytes, position) else {
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
    reader: &mut dyn ReadSeek,
    scan: &ContainerScan,
) -> Result<std::collections::HashMap<u64, BTreeMap<String, String>>, CodecError> {
    let mut out = std::collections::HashMap::new();
    for entry in scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM && entry.name.contains("FusionACTSegmentType")
    }) {
        let bytes = container::decompress_entry(reader, &entry.name)?;
        let mut position = 0usize;
        while position + 4 <= bytes.len() {
            let Some((tag, after_tag)) = lp_ascii(&bytes, position) else {
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
                let Some((name, after_name)) = lp_ascii(&bytes, cursor) else {
                    valid = false;
                    break;
                };
                let Some((guid, after_guid)) = lp_utf16(&bytes, after_name) else {
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
                if let Some((entity, end)) = lp_utf16(&bytes, cursor) {
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

fn lp_ascii(bytes: &[u8], position: usize) -> Option<(String, usize)> {
    let length = u32::from_le_bytes(bytes.get(position..position + 4)?.try_into().ok()?) as usize;
    if !(1..=64).contains(&length) {
        return None;
    }
    let end = position + 4 + length;
    let raw = bytes.get(position + 4..end)?;
    raw.iter()
        .all(|byte| (0x20..0x7f).contains(byte))
        .then(|| (String::from_utf8_lossy(raw).into_owned(), end))
}

fn lp_utf16(bytes: &[u8], position: usize) -> Option<(String, usize)> {
    let length = u32::from_le_bytes(bytes.get(position..position + 4)?.try_into().ok()?) as usize;
    if !(1..=64).contains(&length) {
        return None;
    }
    let end = position + 4 + length * 2;
    let units: Vec<u16> = bytes
        .get(position + 4..end)?
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    Some((String::from_utf16(&units).ok()?, end))
}

fn entity_suffix(value: &str) -> Option<u64> {
    let (_, suffix) = value.split_once('_')?;
    suffix.parse().ok()
}

fn lp_utf16_strings(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    while offset + 4 <= bytes.len() {
        let count = u32::from_le_bytes(
            bytes[offset..offset + 4]
                .try_into()
                .expect("invariant: bytes[offset..offset+4] is a 4-byte slice"),
        ) as usize;
        let byte_len = count.saturating_mul(2);
        if (2..=256).contains(&count) && offset + 4 + byte_len <= bytes.len() {
            let units: Vec<u16> = bytes[offset + 4..offset + 4 + byte_len]
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            if let Ok(value) = String::from_utf16(&units) {
                if value.chars().all(|ch| !ch.is_control()) {
                    out.push((offset, value));
                    offset += 4 + byte_len;
                    continue;
                }
            }
        }
        offset += 1;
    }
    out
}

fn decode_body_map(bytes: &[u8]) -> std::collections::HashMap<u64, (u64, usize)> {
    let mut out = std::collections::HashMap::new();
    let needle: Vec<u8> = "BREP.".encode_utf16().flat_map(u16::to_le_bytes).collect();
    for offset in bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
    {
        let Some(block_end) = offset.checked_sub(16) else {
            continue;
        };
        for count in 1usize..=64 {
            let Some(count_pos) = block_end.checked_sub(count * 16 + 4) else {
                break;
            };
            if u32::from_le_bytes(
                bytes[count_pos..count_pos + 4]
                    .try_into()
                    .expect("invariant: bytes[count_pos..count_pos+4] is a 4-byte slice"),
            ) as usize
                != count
            {
                continue;
            }
            for (index, pair) in bytes[count_pos + 4..count_pos + 4 + count * 16]
                .chunks_exact(16)
                .enumerate()
            {
                out.insert(
                    u64::from_le_bytes(
                        pair[..8]
                            .try_into()
                            .expect("invariant: pair is a 16-byte slice, so pair[..8] is 8 bytes"),
                    ),
                    (
                        u64::from_le_bytes(
                            pair[8..].try_into().expect(
                                "invariant: pair is a 16-byte slice, so pair[8..] is 8 bytes",
                            ),
                        ),
                        count_pos + 4 + index * 16 + 8,
                    ),
                );
            }
            break;
        }
    }
    out
}

fn instance_properties(protein: &[u8]) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(protein)).ok()?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).ok()?;
        if file.name().ends_with("AssetData/InstanceProperties.bin") {
            let mut bytes = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut bytes).ok()?;
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
            let mut bytes = Vec::with_capacity(file.size() as usize);
            file.read_to_end(&mut bytes).ok()?;
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

fn decode_logical_records(bytes: &[u8], stream: &str) -> Vec<Appearance> {
    let starts: Vec<usize> = bytes
        .windows(RECORD_MARKER.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == RECORD_MARKER).then_some(offset))
        .collect();
    starts
        .iter()
        .enumerate()
        .filter_map(|(index, start)| {
            let end = starts.get(index + 1).copied().unwrap_or(bytes.len());
            decode_record(&bytes[*start..end], stream, *start)
        })
        .collect()
}

fn decode_record(record: &[u8], _stream: &str, _offset: usize) -> Option<Appearance> {
    if !record.starts_with(RECORD_MARKER) {
        return None;
    }
    let mut position = RECORD_MARKER.len();
    let schema = take_lp(record, &mut position)?;
    let guid = take_lp(record, &mut position)?;
    let base = take_lp(record, &mut position)?;
    let _base_guid = take_lp(record, &mut position)?;
    let color = match schema.as_str() {
        "GenericSchema" => rgba(
            record,
            position + 112 + generic_connection_delta(record, position),
        ),
        "PrismOpaqueSchema" | "PrismMetalSchema" => rgba(record, position + 8),
        "PrismTransparentSchema" => rgba(record, position + 121),
        "PhysMatSchema"
        | "StructuralMetalSchema"
        | "StructuralPlasticSchema"
        | "ThermalSolidSchema" => None,
        _ => return None,
    };
    if color.is_none()
        && !matches!(
            schema.as_str(),
            "PhysMatSchema"
                | "StructuralMetalSchema"
                | "StructuralPlasticSchema"
                | "ThermalSolidSchema"
        )
    {
        return None;
    }
    let mut properties = BTreeMap::new();
    match schema.as_str() {
        "GenericSchema" => {
            let delta = generic_connection_delta(record, position);
            insert_tagged_scalar(
                &mut properties,
                "reflectivity_at_0deg",
                record,
                position + 171 + delta,
                0.0..=1.0,
            );
            insert_tagged_scalar(
                &mut properties,
                "refraction_index",
                record,
                position + 197 + delta,
                1.0..=4.0,
            );
        }
        "PrismOpaqueSchema" => {
            if let Some(marker) = find(record, b"\x0e\x20\x00\x00", position) {
                insert_scalar(
                    &mut properties,
                    "surface_roughness",
                    record,
                    marker + 4,
                    0.0..=1.0,
                );
            }
        }
        "PrismTransparentSchema" => {
            insert_scalar(
                &mut properties,
                "refraction_index",
                record,
                position + 169,
                1.0..=4.0,
            );
        }
        _ => {}
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
        schema: Some(schema.clone()),
        category: None,
        base_color: color,
        properties,
    })
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

fn insert_scalar(
    out: &mut BTreeMap<String, f64>,
    name: &str,
    bytes: &[u8],
    offset: usize,
    range: std::ops::RangeInclusive<f64>,
) {
    let Some(value) = bytes.get(offset..offset + 8).map(|slice| {
        f64::from_le_bytes(
            slice
                .try_into()
                .expect("invariant: chunks_exact(8) yields 8-byte slices"),
        )
    }) else {
        return;
    };
    if value.is_finite() && range.contains(&value) {
        out.insert(name.into(), value);
    }
}

fn insert_tagged_scalar(
    out: &mut BTreeMap<String, f64>,
    name: &str,
    bytes: &[u8],
    offset: usize,
    range: std::ops::RangeInclusive<f64>,
) {
    if bytes.get(offset..offset + 4) == Some(b"\x0c\x00\x00\x00") {
        insert_scalar(out, name, bytes, offset + 4, range);
    }
}

fn take_lp(bytes: &[u8], position: &mut usize) -> Option<String> {
    let length = u32::from_le_bytes(bytes.get(*position..*position + 4)?.try_into().ok()?) as usize;
    *position += 4;
    let value = String::from_utf8(bytes.get(*position..*position + length)?.to_vec()).ok()?;
    *position += length;
    Some(value)
}

fn rgba(bytes: &[u8], offset: usize) -> Option<Color> {
    let read = |at: usize| Some(f64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?));
    let [r, g, b, a] = [
        read(offset)?,
        read(offset + 8)?,
        read(offset + 16)?,
        read(offset + 24)?,
    ];
    if ![r, g, b, a].iter().all(|value| value.is_finite())
        || ![r, g, b].iter().all(|value| (0.0..=1.0).contains(value))
        || (a - 1.0).abs() > 1e-3
    {
        return None;
    }
    Some(Color {
        r: r as f32,
        g: g as f32,
        b: b as f32,
        a: a as f32,
    })
}
