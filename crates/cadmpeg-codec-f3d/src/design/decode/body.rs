// SPDX-License-Identifier: Apache-2.0
//! Parse body members, bounds, bindings, and visibility.

use crate::bytes::lp_ascii_filtered;
use crate::container::{role, ContainerScan};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::RECIPES;
use crate::ids::{self, native_stream};
use crate::records::{
    BodyNativeKey, ConstructionRecipe, ConstructionRecipeKind, DesignBodyBinding, DesignBodyBounds,
    DesignBodyMember, DesignEntityHeader, DesignObjectKind,
};
use cadmpeg_ir::codec::CodecError;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::wire::le::{f64_at, u32_at, u32_at as read_u32, u64_at as read_u64};
use std::collections::HashMap;

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub fn decode_body_members(scan: &ContainerScan) -> Result<Vec<DesignBodyMember>, CodecError> {
    let mut out = Vec::new();
    let mut prefix = Vec::new();
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    prefix.extend_from_slice(&0u16.to_le_bytes());
    prefix.extend_from_slice(&10u32.to_le_bytes());
    prefix.extend_from_slice(b"BodiesRoot");
    for entry in scan
        .entries
        .iter()
        .filter(|entry| entry.role == role::BULKSTREAM && entry.name.contains("Design"))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        else {
            continue;
        };
        let count_offset = start + prefix.len();
        let Some(count_raw) = bytes.get(count_offset..count_offset + 4) else {
            continue;
        };
        let count =
            usize::try_from(u32::from_le_bytes(count_raw.try_into().expect(
                "invariant: count_raw is a 4-byte slice from bytes.get(range) of length 4",
            )))
            .unwrap_or(usize::MAX);
        if count > 100_000 {
            continue;
        }
        let mut cursor = count_offset + 4;
        let mut decoded = Vec::with_capacity(count);
        for _ in 0..count {
            if bytes.get(cursor) != Some(&1) {
                decoded.clear();
                break;
            }
            let Some(id_raw) = bytes.get(cursor + 1..cursor + 9) else {
                decoded.clear();
                break;
            };
            let Some(flags_raw) = bytes.get(cursor + 9..cursor + 11) else {
                decoded.clear();
                break;
            };
            decoded.push(DesignBodyMember {
                id: ids::native_design_body_member_id(&entry.name, cursor),
                byte_offset: cursor as u64,
                entity_suffix: u64::from_le_bytes(id_raw.try_into().expect(
                    "invariant: id_raw is an 8-byte slice from bytes.get(range) of length 8",
                )),
                flags: u16::from_le_bytes(flags_raw.try_into().expect(
                    "invariant: flags_raw is a 2-byte slice from bytes.get(range) of length 2",
                )),
            });
            cursor += 11;
        }
        if decoded.len() == count && bytes.get(cursor) == Some(&0) {
            out.extend(decoded);
        }
    }
    Ok(out)
}

/// Decode the three consecutive indexed records that cache each Design body's
/// axis-aligned model-space bounds.
pub fn decode_body_bounds(
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignBodyBounds>, CodecError> {
    let mut out = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| entity.object_kind == Some(DesignObjectKind::Body))
    {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        let Some(entry) = scan.entries.iter().find(|entry| {
            entry.role == role::BULKSTREAM
                && entry.name.contains("Design")
                && stream == ids::native_scope(&entry.name)
        }) else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = usize::try_from(entity.byte_offset).ok() else {
            continue;
        };
        let end = entities
            .iter()
            .filter(|candidate| {
                native_stream(&candidate.id) == Some(stream)
                    && candidate.byte_offset > entity.byte_offset
            })
            .filter_map(|candidate| usize::try_from(candidate.byte_offset).ok())
            .min()
            .unwrap_or(bytes.len());
        let Ok(record_index) = u32::try_from(entity.entity_suffix) else {
            continue;
        };
        let Some(record_indices) = record_index
            .checked_add(1)
            .zip(record_index.checked_add(2))
            .zip(record_index.checked_add(3))
            .map(|((first, second), third)| [first, second, third])
        else {
            continue;
        };
        let mut record_offsets = Vec::with_capacity(3);
        for wanted in record_indices {
            let matches = indexed_headers_in(bytes, start, end)
                .filter(|(_, record_index)| *record_index == wanted)
                .map(|(offset, _)| offset)
                .collect::<Vec<_>>();
            let [offset] = matches.as_slice() else {
                record_offsets.clear();
                break;
            };
            record_offsets.push(*offset);
        }
        let [first, second, third] = record_offsets.as_slice() else {
            continue;
        };
        if !(first < second && second < third) {
            continue;
        }
        let third_end = next_indexed_record_offset(bytes, third.saturating_add(11))
            .filter(|offset| *offset <= end)
            .unwrap_or(end);
        let intervals = [(*first, *second), (*second, *third), (*third, third_end)];
        let mut repeated = body_bound_candidates(bytes, intervals[0].0, intervals[0].1)
            .filter_map(|(marker_offset, values)| {
                let frame = bytes.get(marker_offset..marker_offset + 49)?;
                let mut value_offsets = [marker_offset + 1, 0, 0];
                for (ordinal, (record_start, record_end)) in
                    intervals.iter().copied().enumerate().skip(1)
                {
                    let matches = body_bound_candidates(bytes, record_start, record_end)
                        .filter(|(offset, _)| {
                            bytes.get(*offset..offset.saturating_add(49)) == Some(frame)
                        })
                        .map(|(offset, _)| offset + 1)
                        .collect::<Vec<_>>();
                    let [offset] = matches.as_slice() else {
                        return None;
                    };
                    value_offsets[ordinal] = *offset;
                }
                Some((values, value_offsets))
            })
            .collect::<Vec<_>>();
        repeated.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        let [(values, value_offsets)] = repeated.as_slice() else {
            continue;
        };
        out.push(DesignBodyBounds {
            id: ids::native_design_body_bounds_id(&entry.name, entity.byte_offset),
            entity_suffix: entity.entity_suffix,
            entity_byte_offset: entity.byte_offset,
            record_indices,
            record_byte_offsets: [*first as u64, *second as u64, *third as u64],
            value_byte_offsets: value_offsets.map(|offset| offset as u64),
            body_binding_ids: Vec::new(),
            maximum: Point3::new(values[0] * 10.0, values[1] * 10.0, values[2] * 10.0),
            minimum: Point3::new(values[3] * 10.0, values[4] * 10.0, values[5] * 10.0),
        });
    }
    out.sort_by_key(|bounds| bounds.id.clone());
    Ok(out)
}

fn indexed_headers_in(
    bytes: &[u8],
    mut position: usize,
    end: usize,
) -> impl Iterator<Item = (usize, u32)> + '_ {
    std::iter::from_fn(move || {
        while position + 11 <= end {
            let at = position;
            position += 1;
            let Some((class_tag, after_tag)) =
                lp_ascii_filtered(bytes, at, 0..=2000, u8::is_ascii_graphic)
            else {
                continue;
            };
            if class_tag.len() == 3 && class_tag.bytes().all(|byte| byte.is_ascii_digit()) {
                let Some(record_index) = u32_at(bytes, after_tag) else {
                    continue;
                };
                return Some((at, record_index));
            }
        }
        None
    })
}

pub(crate) fn body_bound_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> impl Iterator<Item = (usize, [f64; 6])> + '_ {
    (start..end.saturating_sub(48)).filter_map(move |offset| {
        if bytes.get(offset) != Some(&1) {
            return None;
        }
        let values = [
            f64_at(bytes, offset + 1)?,
            f64_at(bytes, offset + 9)?,
            f64_at(bytes, offset + 17)?,
            f64_at(bytes, offset + 25)?,
            f64_at(bytes, offset + 33)?,
            f64_at(bytes, offset + 41)?,
        ];
        (values.iter().all(|value| value.is_finite())
            && (0..3).all(|axis| values[axis] >= values[axis + 3])
            && (0..3).any(|axis| values[axis] > values[axis + 3]))
        .then_some((offset, values))
    })
}

pub(crate) fn object_kind(name: &str) -> DesignObjectKind {
    match name {
        "Fusion" => DesignObjectKind::Fusion,
        "Body" => DesignObjectKind::Body,
        "Component" => DesignObjectKind::Component,
        "Geometry" => DesignObjectKind::Geometry,
        "MSketch" => DesignObjectKind::Sketch,
        "Dimension" => DesignObjectKind::Dimension,
        "Scene" => DesignObjectKind::Scene,
        "EntityTracking" => DesignObjectKind::EntityTracking,
        "CommonData" => DesignObjectKind::CommonData,
        _ => DesignObjectKind::Other(name.to_owned()),
    }
}

pub(crate) fn decode_stream(bytes: &[u8], stream: &str, out: &mut Vec<ConstructionRecipe>) {
    let mut counters: HashMap<(ConstructionRecipeKind, Option<String>), u32> = HashMap::new();
    for &(name, kind) in RECIPES {
        let mut cursor = 0;
        while let Some(relative) = bytes[cursor..].windows(name.len()).position(|w| w == name) {
            let offset = cursor + relative;
            cursor = offset + 1;
            if kind == ConstructionRecipeKind::Face
                && offset >= 8
                && &bytes[offset - 8..offset] == b"bounded_"
            {
                continue;
            }
            let framed_name = offset
                .checked_sub(4)
                .and_then(|at| u32_at(bytes, at))
                .and_then(|length| usize::try_from(length).ok())
                == Some(name.len());
            if !framed_name {
                continue;
            }
            let design_id_field = recipe_design_id(bytes, offset, name);
            let design_id = design_id_field.as_ref().map(|field| field.0.clone());
            let key = (kind, design_id.clone());
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            let record_index_offset = offset.checked_sub(16);
            let record_index = record_index_offset
                .and_then(|at| bytes.get(at..at + 4))
                .map(|raw| {
                    i32::from_le_bytes(
                        raw.try_into()
                            .expect("invariant: bytes.get(at..at+4) is a 4-byte slice"),
                    )
                })
                .unwrap_or_default();
            out.push(ConstructionRecipe {
                id: ids::native_construction_recipe_id(stream, offset),
                byte_offset: offset as u64,
                record_index_offset: record_index_offset.map(|offset| offset as u64),
                kind,
                design_id,
                design_id_offset: design_id_field.as_ref().map(|field| field.1 as u64),
                recipe_index,
                record_index,
            });
        }
    }
    out.sort_by_key(|recipe| recipe.record_index);
}

fn recipe_design_id(bytes: &[u8], offset: usize, name: &[u8]) -> Option<(String, usize)> {
    let id_end = offset.checked_sub(20)?;
    for length in 1..=8usize {
        let Some(length_at) = id_end.checked_sub(4 + length) else {
            continue;
        };
        if let Some((id, value_offset)) = ascii_id_at(bytes, length_at) {
            if value_offset.checked_add(id.len()) == Some(id_end) {
                return Some((id, value_offset));
            }
        }
    }
    if offset >= 23 {
        let candidate = bytes.get(offset - 23..offset - 20)?;
        if candidate.iter().all(u8::is_ascii_digit) {
            return Some((String::from_utf8_lossy(candidate).into_owned(), offset - 23));
        }
    }
    ascii_id_at(bytes, offset + name.len() + 8)
}

fn ascii_id_at(bytes: &[u8], length_offset: usize) -> Option<(String, usize)> {
    let length = usize::try_from(u32::from_le_bytes(
        bytes
            .get(length_offset..length_offset + 4)?
            .try_into()
            .ok()?,
    ))
    .ok()?;
    if !(1..=8).contains(&length) {
        return None;
    }
    let value = bytes.get(length_offset + 4..length_offset + 4 + length)?;
    value.iter().all(u8::is_ascii_alphanumeric).then(|| {
        (
            String::from_utf8_lossy(value).into_owned(),
            length_offset + 4,
        )
    })
}

/// One `(asm_body_key, entity_suffix)` pair from a Design `BulkStream` BREP
/// body-map record, with the named B-rep blob the key resolves in and the
/// suffix's byte offset for native patching.
pub(crate) struct BodyBinding {
    /// Basename of the B-rep blob entry the ASM key resolves in.
    pub blob_name: String,
    /// Byte offset of the BREP blob name's UTF-16LE code units.
    pub blob_name_offset: usize,
    /// Number of pairs in the enclosing map.
    pub pair_count: u32,
    /// Zero-based position in the enclosing map.
    pub pair_ordinal: u32,
    /// The referenced ASM body key.
    pub asm_key: u64,
    /// Byte offset of `asm_key` within the stream.
    pub asm_key_offset: usize,
    /// The body's design-entity suffix.
    pub entity_suffix: u64,
    /// Byte offset of `entity_suffix` within the stream.
    pub entity_suffix_offset: usize,
}

/// Parse every BREP body-map record in a Design `BulkStream`: a `u32` pair
/// count, `count` pairs of `(u64 asm_body_key, u64 entity_suffix)`, the
/// trailing record ref and pad, then the length-prefixed UTF-16 blob name
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
pub(crate) fn body_bindings(bytes: &[u8]) -> Vec<BodyBinding> {
    let needle: Vec<u8> = "BREP.".encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut out = Vec::new();
    for offset in bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(offset, window)| (window == needle).then_some(offset))
    {
        let Some(name_chars) = offset
            .checked_sub(4)
            .and_then(|at| read_u32(bytes, at))
            .map(|chars| chars as usize)
        else {
            continue;
        };
        let Some(blob_name) = bytes
            .get(offset..offset + name_chars * 2)
            .map(utf16_le_string)
        else {
            continue;
        };
        // 16 bytes separate the pairs from the name: the 12-byte record tail
        // and the name's u32 length prefix.
        let Some(pairs_end) = offset.checked_sub(16) else {
            continue;
        };
        // The pair count precedes the pairs; scanning ascending is unambiguous
        // because the high halves of the little-endian ids are zero.
        for count in 1usize..=64 {
            let span = 16 * count;
            let Some(count_at) = pairs_end.checked_sub(span + 4) else {
                break;
            };
            if read_u32(bytes, count_at) != Some(count as u32) {
                continue;
            }
            for pair in 0..count {
                let at = count_at + 4 + pair * 16;
                if let (Some(key), Some(suffix)) = (read_u64(bytes, at), read_u64(bytes, at + 8)) {
                    out.push(BodyBinding {
                        blob_name: blob_name.clone(),
                        blob_name_offset: offset,
                        pair_count: count as u32,
                        pair_ordinal: pair as u32,
                        asm_key: key,
                        asm_key_offset: at,
                        entity_suffix: suffix,
                        entity_suffix_offset: at + 8,
                    });
                }
            }
            break;
        }
    }
    out
}

/// Decode every ordered Design BREP body-map pair and resolve each pair in its
/// named blob's body-selector namespace.
pub fn decode_design_body_bindings(
    scan: &ContainerScan,
    active_brep_entry: Option<&str>,
    body_keys: &[BodyNativeKey],
) -> Result<Vec<DesignBodyBinding>, CodecError> {
    let active_basename = active_brep_entry.and_then(|entry| entry.rsplit('/').next());
    let mut out = Vec::new();
    for entry in scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM
            && entry.name.contains("Design")
            && scan
                .asset_folder
                .as_ref()
                .is_none_or(|folder| entry.name.starts_with(&format!("{folder}/")))
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        for binding in body_bindings(bytes) {
            let source_bodies = body_keys
                .iter()
                .filter(|key| {
                    key.source_brep.as_deref().map_or_else(
                        || active_basename == Some(binding.blob_name.as_str()),
                        |source| source == binding.blob_name,
                    )
                })
                .collect::<Vec<_>>();
            let direct = source_bodies
                .iter()
                .filter(|key| key.asm_body_key == Some(binding.asm_key))
                .map(|key| key.body.clone())
                .collect::<Vec<_>>();
            let body = match direct.as_slice() {
                [body] => Some(body.clone()),
                [] if source_bodies.iter().all(|key| key.asm_body_key.is_none()) => {
                    let ordinal = u32::try_from(binding.asm_key).ok();
                    let ordinal_matches = source_bodies
                        .iter()
                        .filter(|key| Some(key.body_ordinal) == ordinal)
                        .map(|key| key.body.clone())
                        .collect::<Vec<_>>();
                    match ordinal_matches.as_slice() {
                        [body] => Some(body.clone()),
                        _ => None,
                    }
                }
                _ => None,
            };
            out.push(DesignBodyBinding {
                id: ids::native_design_body_binding_id(&entry.name, binding.asm_key_offset),
                stream: entry.name.clone(),
                pair_count: binding.pair_count,
                pair_ordinal: binding.pair_ordinal,
                asm_body_key: binding.asm_key,
                asm_body_key_offset: binding.asm_key_offset as u64,
                entity_suffix: binding.entity_suffix,
                entity_suffix_offset: binding.entity_suffix_offset as u64,
                blob_name: binding.blob_name,
                blob_name_offset: binding.blob_name_offset as u64,
                body,
            });
        }
    }
    out.sort_by_key(|binding| binding.id.clone());
    Ok(out)
}

/// Bind each body cache to every BREP map pair carrying the same Design entity
/// suffix in the same stream.
pub fn bind_body_bounds(bounds: &mut [DesignBodyBounds], bindings: &[DesignBodyBinding]) {
    for bounds in bounds {
        let Some(stream) = native_stream(&bounds.id) else {
            continue;
        };
        let mut matches = bindings
            .iter()
            .filter(|binding| {
                stream == ids::native_scope(&binding.stream)
                    && binding.entity_suffix == bounds.entity_suffix
            })
            .collect::<Vec<_>>();
        matches.sort_by_key(|binding| binding.asm_body_key_offset);
        bounds.body_binding_ids = matches
            .into_iter()
            .map(|binding| binding.id.clone())
            .collect();
    }
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// Each BREP body-map record resolves blob-qualified body selectors to Design
/// entity suffixes, and each entity's browser-node record carries a hidden flag
/// directly after the node GUID
/// ([spec §8.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#81-design-metadata)).
/// The result maps each blob and body selector to its display visibility;
/// bodies without records are absent.
#[derive(Debug, Clone)]
pub(crate) struct DecodedBodyVisibility {
    pub stream: String,
    pub byte_offset: u64,
    pub asm_body_key_offset: u64,
    pub entity_suffix: u64,
    pub visible: bool,
}

pub(crate) fn decode_all_body_visibility(
    scan: &ContainerScan,
) -> Result<HashMap<(String, u64), DecodedBodyVisibility>, CodecError> {
    let mut out = HashMap::new();
    for entry in scan.entries.iter().filter(|entry| {
        entry.role == role::BULKSTREAM
            && entry.name.contains("Design")
            && scan
                .asset_folder
                .as_ref()
                .is_none_or(|folder| entry.name.starts_with(&format!("{folder}/")))
    }) {
        let bytes = scan.entry_bytes(&entry.name)?;
        let hidden_by_entity = browser_node_hidden_flags(bytes);
        for binding in body_bindings(bytes) {
            if let Some(node) = hidden_by_entity.get(&binding.entity_suffix) {
                out.insert(
                    (binding.blob_name, binding.asm_key),
                    DecodedBodyVisibility {
                        stream: entry.name.clone(),
                        byte_offset: node.byte_offset,
                        asm_body_key_offset: binding.asm_key_offset as u64,
                        entity_suffix: binding.entity_suffix,
                        visible: !node.hidden,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// Scan for browser-node records: a length-prefixed 36-character UTF-16 GUID,
/// one hidden-flag byte, the `01 01` marker, and the `u64` design-entity
/// suffix.
#[derive(Debug, Clone, Copy)]
struct BrowserNodeVisibility {
    byte_offset: u64,
    hidden: bool,
}

fn browser_node_hidden_flags(bytes: &[u8]) -> HashMap<u64, BrowserNodeVisibility> {
    const GUID_CHARS: usize = 36;
    const GUID_BYTES: usize = GUID_CHARS * 2;
    let mut out = HashMap::new();
    let mut at = 0usize;
    while at + 4 + GUID_BYTES + 3 + 8 <= bytes.len() {
        if read_u32(bytes, at) != Some(GUID_CHARS as u32)
            || !is_utf16_guid(&bytes[at + 4..at + 4 + GUID_BYTES])
        {
            at += 1;
            continue;
        }
        let flag_at = at + 4 + GUID_BYTES;
        if bytes.get(flag_at + 1..flag_at + 3) == Some(&[0x01, 0x01]) {
            if let (flag @ (0 | 1), Some(member)) = (bytes[flag_at], read_u64(bytes, flag_at + 3)) {
                out.insert(
                    member,
                    BrowserNodeVisibility {
                        byte_offset: flag_at as u64,
                        hidden: flag == 1,
                    },
                );
            }
        }
        at += 1;
    }
    out
}

fn utf16_le_string(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

fn is_utf16_guid(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(2)
        .all(|pair| pair[1] == 0 && (pair[0].is_ascii_hexdigit() || pair[0] == b'-'))
}
