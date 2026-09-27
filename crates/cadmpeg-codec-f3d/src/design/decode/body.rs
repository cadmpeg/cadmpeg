// SPDX-License-Identifier: Apache-2.0
//! Parse body members, bounds, bindings, and visibility.

use cadmpeg_core::container::ContainerRole;

use crate::bytes::take_reference;
use crate::container::ContainerScan;
use crate::design::decode::text::{design_record_id_charged, lp_utf16_bounded_charged};
use crate::design::decode::sketch::next_indexed_record_offset;
use crate::design::decode::sketch::native_scope_charged;
use crate::design::RECIPES;
use crate::ids::native_stream;
use crate::layout::indexed_design_record_header;
use crate::records::{
    bodies::{DesignBodyBinding, DesignBodyBounds, DesignBodyMember},
    entity_header::{DesignEntityHeader, DESIGN_MODULE_BODY},
    recipes::{ConstructionRecipe, ConstructionRecipeKind, ConstructionRecipeSelector},
};
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_core::bytes::find_from;
use cadmpeg_core::decode::{bounded_len, index_from_u32, u64_from_index, DecodeContext, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::scalar::FiniteReal;
use std::collections::{HashMap, HashSet};

/// Decode the `BodiesRoot` member list following the doubled `BodiesRoot`
/// marker in each design `BulkStream` entry in `scan`: each member's entity
/// suffix and flags. The decode is rejected (no members returned for that
/// stream) unless the declared count is fully consumed and immediately
/// followed by a zero byte.
pub(crate) fn decode_body_members(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<DesignBodyMember>, CodecError> {
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
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(start) = bytes
            .windows(prefix.len())
            .position(|window| window == prefix)
        else {
            continue;
        };
        let count_offset = start + prefix.len();
        let mut view = View::over_retained(bytes);
        if view.seek(count_offset).is_none() {
            continue;
        }
        let Some(count) = view.u32_le().map(index_from_u32) else {
            continue;
        };
        if count > 100_000 {
            continue;
        }
        let Some(count) = bounded_len(u64_from_index(count), 11, view.remaining()) else {
            continue;
        };
        ctx.charge_collection_items(u64_from_index(count), "f3d body members")?;
        let mut decoded = Vec::new();
        decoded.try_reserve(count).map_err(|_| {
            ctx.refuse_codec_limit("f3d body members allocation", 0, 1)
        })?;
        for _ in 0..count {
            let cursor = view.position();
            if view.u8() != Some(1) {
                decoded.clear();
                break;
            }
            let Some(entity_suffix) = view.u64_le() else {
                decoded.clear();
                break;
            };
            let Some(flags) = view.u16_le() else {
                decoded.clear();
                break;
            };
            let id = design_record_id_charged(
                ctx,
                &entry.name,
                ":design-body-member#",
                u64_from_index(cursor),
                "f3d body member identifier",
                "f3d body member identifier allocation",
            )?;
            decoded.push(DesignBodyMember {
                id,
                byte_offset: u64_from_index(cursor),
                entity_suffix,
                flags,
            });
        }
        if decoded.len() == count && bytes.get(view.position()) == Some(&0) {
            ctx.charge_collection_items(u64_from_index(decoded.len()), "f3d decoded body members")?;
            out.try_reserve(decoded.len()).map_err(|_| {
                ctx.refuse_codec_limit("f3d decoded body members allocation", 0, 1)
            })?;
            out.extend(decoded);
        }
    }
    Ok(out)
}

/// Decode the three consecutive indexed records that cache each Design body's
/// axis-aligned model-space bounds.
pub(crate) fn decode_body_bounds(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    entities: &[DesignEntityHeader],
) -> Result<Vec<DesignBodyBounds>, CodecError> {
    let mut out = Vec::new();
    for entity in entities
        .iter()
        .filter(|entity| entity.module() == Some(DESIGN_MODULE_BODY))
    {
        let Some(stream) = native_stream(&entity.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
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
        let Ok(record_index) = u32::try_from(entity.entity_id.suffix()) else {
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
        let mut record_offsets = [0; 3];
        let mut unique = true;
        for (ordinal, wanted) in record_indices.into_iter().enumerate() {
            let mut matches = indexed_headers_in(bytes, start, end)
                .filter(|(_, record_index)| *record_index == wanted)
                .map(|(offset, _)| offset);
            match (matches.next(), matches.next()) {
                (Some(offset), None) => record_offsets[ordinal] = offset,
                _ => {
                    unique = false;
                    break;
                }
            }
        }
        if !unique {
            continue;
        }
        let [first, second, third] = record_offsets;
        if !(first < second && second < third) {
            continue;
        }
        let third_end = next_indexed_record_offset(bytes, third.saturating_add(11))
            .filter(|offset| *offset <= end)
            .unwrap_or(end);
        let intervals = [(first, second), (second, third), (third, third_end)];
        let mut repeated = body_bound_candidates(bytes, intervals[0].0, intervals[0].1)
            .filter_map(|(marker_offset, values)| {
                let frame = bytes.get(marker_offset..marker_offset + 49)?;
                let mut value_offsets = [marker_offset + 1, 0, 0];
                for (ordinal, (record_start, record_end)) in
                    intervals.iter().copied().enumerate().skip(1)
                {
                    let mut matches = body_bound_candidates(bytes, record_start, record_end)
                        .filter(|(offset, _)| {
                            bytes.get(*offset..offset.saturating_add(49)) == Some(frame)
                        })
                        .map(|(offset, _)| offset + 1);
                    value_offsets[ordinal] = match (matches.next(), matches.next()) {
                        (Some(offset), None) => offset,
                        _ => return None,
                    };
                }
                Some((values, value_offsets))
            });
        let Some((values, value_offsets)) = repeated.next() else {
            continue;
        };
        if repeated.any(|candidate| candidate.0 != values || candidate.1 != value_offsets) {
            continue;
        }
        let corner = |start: usize| {
            FinitePoint3::new(Point3::new(
                values[start].get() * 10.0,
                values[start + 1].get() * 10.0,
                values[start + 2].get() * 10.0,
            ))
            .ok_or_else(|| {
                CodecError::Malformed("scene bounds maximum and minimum must be finite".into())
            })
        };
        let record = DesignBodyBounds::from_parts(crate::records::bodies::DesignBodyBoundsWire {
                id: design_record_id_charged(
                    ctx,
                    &entry.name,
                    ":design-body-bounds#",
                    entity.byte_offset,
                    "f3d body record identifier",
                    "f3d body record identifier allocation",
                )?,
                entity_suffix: entity.entity_id.suffix(),
                entity_byte_offset: entity.byte_offset,
                record_indices,
                record_byte_offsets: [u64_from_index(first), u64_from_index(second), u64_from_index(third)],
                value_byte_offsets: value_offsets.map(u64_from_index),
                body_binding_ids: Vec::new(),
                maximum: corner(0)?,
                minimum: corner(3)?,
            }).map_err(CodecError::Malformed)?;
        ctx.charge_collection_items(1, "f3d body bounds")?;
        out.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("f3d body bounds allocation", 0, 1)
        })?;
        out.push(record);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
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
            if View::u32_le_at(bytes, at) != Some(3) {
                continue;
            }
            let Some(class_tag) = bytes.get(at + 4..at + 7) else {
                continue;
            };
            if !class_tag.iter().all(u8::is_ascii_digit) {
                continue;
            }
            let Some(record_index) = View::u32_le_at(bytes, at + 7) else {
                continue;
            };
            return Some((at, record_index));
        }
        None
    })
}

fn body_bound_candidates(
    bytes: &[u8],
    start: usize,
    end: usize,
) -> impl Iterator<Item = (usize, [FiniteReal; 6])> + '_ {
    (start..end.saturating_sub(48)).filter_map(move |offset| {
        if bytes.get(offset) != Some(&1) {
            return None;
        }
        let values = [
            FiniteReal::new(View::f64_le_at(bytes, offset + 1)?)?,
            FiniteReal::new(View::f64_le_at(bytes, offset + 9)?)?,
            FiniteReal::new(View::f64_le_at(bytes, offset + 17)?)?,
            FiniteReal::new(View::f64_le_at(bytes, offset + 25)?)?,
            FiniteReal::new(View::f64_le_at(bytes, offset + 33)?)?,
            FiniteReal::new(View::f64_le_at(bytes, offset + 41)?)?,
        ];
        ((0..3).all(|axis| values[axis] >= values[axis + 3])
            && (0..3).any(|axis| values[axis] > values[axis + 3]))
        .then_some((offset, values))
    })
}

pub(super) fn decode_stream(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    out: &mut Vec<ConstructionRecipe>,
) -> Result<(), CodecError> {
    let mut counters: HashMap<(ConstructionRecipeKind, Option<String>), u32> = HashMap::new();
    for &(name, kind) in RECIPES {
        let mut cursor = 0;
        while let Some(offset) = find_from(bytes, name, cursor) {
            cursor = offset + 1;
            if kind == ConstructionRecipeKind::Face
                && offset >= 8
                && &bytes[offset - 8..offset] == b"bounded_"
            {
                continue;
            }
            let framed_name = offset
                .checked_sub(4)
                .and_then(|at| View::u32_le_at(bytes, at))
                .and_then(|length| usize::try_from(length).ok())
                == Some(name.len());
            if !framed_name {
                continue;
            }
            let design = recipe_design_id(bytes, offset, name).map(|(value, design_id_at)| {
                let selector = design_id_at
                    .checked_add(value.len())
                    .and_then(|selector_at| {
                        Some(ConstructionRecipeSelector {
                            value: View::u32_le_at(bytes, selector_at)?,
                            byte_offset: u64::try_from(selector_at).ok()?,
                        })
                    });
                crate::records::recipes::ConstructionRecipeDesign {
                    id: crate::records::identity::RecordedValue {
                        value,
                        offset: design_id_at as u64,
                    },
                    selector,
                }
            });
            let key = (kind, design.as_ref().map(|design| design.id.value.clone()));
            if !counters.contains_key(&key) {
                ctx.charge_collection_items(1, "f3d construction recipe counters")?;
                counters.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d construction recipe counters allocation", 0, 1)
                })?;
            }
            let counter = counters.entry(key).or_default();
            let recipe_index = *counter;
            *counter += 1;
            // The record index word precedes the family marker by sixteen
            // bytes. A marker within the first sixteen bytes of the stream has
            // no such word, so the stream states no record index for it; the
            // word itself always lies inside `bytes` when the marker does.
            let record_index = offset.checked_sub(16).and_then(|at| {
                Some(crate::records::identity::RecordedValue {
                    value: View::i32_le_at(bytes, at)?,
                    offset: u64_from_index(at),
                })
            });
            let recipe = ConstructionRecipe {
                id: design_record_id_charged(
                    ctx,
                    stream,
                    ":construction-recipe#",
                    u64_from_index(offset),
                    "f3d body record identifier",
                    "f3d body record identifier allocation",
                )?,
                byte_offset: u64_from_index(offset),
                kind,
                design,
                recipe_index,
                record_index,
            };
            ctx.charge_collection_items(1, "f3d construction recipes")?;
            out.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d construction recipes allocation", 0, 1)
            })?;
            out.push(recipe);
        }
    }
    out.sort_by_key(|recipe| recipe.record_index.map(|index| index.value));
    Ok(())
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
    let length = usize::try_from(View::u32_le_at(bytes, length_offset)?).ok()?;
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
    /// The referenced ASM body key.
    pub(crate) asm_key: u64,
    /// Byte offset of `asm_key` within the stream.
    pub(crate) asm_key_offset: usize,
    /// The body's design-entity suffix.
    pub(crate) entity_suffix: u64,
}

impl BodyBinding {
    /// Byte offset of `entity_suffix`, which follows `asm_key` in the pair.
    pub(crate) fn entity_suffix_offset(&self) -> usize {
        self.asm_key_offset + 8
    }
}

/// One exactly framed Design body-map record.
///
/// The record owns the blob name and its location, and its ordered `bindings`
/// are the map's pairs: a binding's ordinal is its index and the pair count is
/// `bindings.len()`.
struct BodyMapRecord {
    blob_name: String,
    /// Byte offset of the BREP blob name's UTF-16LE code units.
    blob_name_offset: usize,
    bindings: Vec<BodyBinding>,
}

fn entity_has_type(meta: &crate::metastream::MetaStream, entity: u64, type_guid: &str) -> bool {
    meta.types.iter().any(|design_type| {
        design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(type_guid)
            && design_type
                .entities
                .values()
                .any(|registered| *registered == entity)
    })
}

#[derive(Clone, Copy)]
enum ReferencePadding {
    None,
    TwoZeros,
}

impl ReferencePadding {
    fn trailing_zeros(self) -> usize {
        match self {
            Self::None => 0,
            Self::TwoZeros => 2,
        }
    }
}

struct LocalReferenceCandidate {
    target: u64,
    end: usize,
    inline_type_guid: Option<String>,
    padding: ReferencePadding,
}

fn local_reference_candidates(
    bytes: &[u8],
    at: usize,
    allow_extra_zero: bool,
) -> Vec<LocalReferenceCandidate> {
    let mut candidates = Vec::new();
    let mut end = at;
    if let Some(reference) = take_reference(bytes, &mut end) {
        if let Some((target, inline_type_guid)) = reference.into_local() {
            candidates.push(LocalReferenceCandidate {
                target,
                end,
                inline_type_guid: inline_type_guid.clone(),
                padding: ReferencePadding::TwoZeros,
            });
            if allow_extra_zero && bytes.get(end) == Some(&0) {
                candidates.push(LocalReferenceCandidate {
                    target,
                    end: end + 1,
                    inline_type_guid,
                    padding: ReferencePadding::TwoZeros,
                });
            }
        }
    }
    if at.checked_add(2).and_then(|end| bytes.get(at..end)) == Some(&[1, 1]) {
        if let (Some(target_at), Some(end)) = (at.checked_add(2), at.checked_add(10)) {
            if let Some(target) = View::u64_le_at(bytes, target_at) {
                candidates.push(LocalReferenceCandidate {
                    target,
                    end,
                    inline_type_guid: None,
                    padding: ReferencePadding::None,
                });
                if allow_extra_zero && bytes.get(end) == Some(&0) {
                    candidates.push(LocalReferenceCandidate {
                        target,
                        end: end + 1,
                        inline_type_guid: None,
                        padding: ReferencePadding::None,
                    });
                }
            }
        }
    }
    candidates
}

fn reference_has_type(
    meta: &crate::metastream::MetaStream,
    reference: &LocalReferenceCandidate,
    expected_type_guid: &str,
) -> bool {
    reference
        .inline_type_guid
        .as_deref()
        .is_none_or(|guid| guid.eq_ignore_ascii_case(expected_type_guid))
        && entity_has_type(meta, reference.target, expected_type_guid)
}

/// Parse every exactly framed sibling body-map record that binds an `.smb`
/// snapshot. The carrier uses a bare entity header in every serializer band.
fn snapshot_body_map_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyMapRecord>, CodecError> {
    let frames = crate::metastream::primary_record_frames(meta, bytes.len())?;
    let mut primary_by_entity = HashMap::new();
    for (ordinal, frame) in frames.iter().enumerate() {
        if !primary_by_entity.contains_key(&frame.entity_id) {
            ctx.charge_collection_items(1, "f3d snapshot body-map primary index")?;
            primary_by_entity.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d snapshot body-map primary index allocation", 0, 1)
            })?;
        }
        primary_by_entity.insert(frame.entity_id, ordinal);
    }
    let mut out = Vec::new();
    for (type_ordinal, design_type) in meta.types.iter().enumerate() {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if !crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_VERSIONS
            .contains(&design_type.version)
        {
            return Err(CodecError::NotImplemented(format!(
                "unsupported F3D Design snapshot body-map carrier version {}",
                design_type.version
            )));
        }
        if design_type.module != DESIGN_MODULE_BODY
            || !design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| {
                    base.eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID)
                })
        {
            return Err(crate::error::malformed(
                "F3D Design snapshot body-map carrier has incompatible registration metadata",
            ));
        }
        let class_tag = u32::try_from(type_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
            .filter(|tag| *tag <= 999)
            .ok_or_else(|| {
                crate::error::malformed(
                    "F3D Design snapshot body-map class tag is not three digits",
                )
            })?
            .to_string();
        for &entity in design_type.entities.values() {
            let Some(&frame_ordinal) = primary_by_entity.get(&entity) else {
                return Err(crate::error::malformed(format!(
                    "F3D Design snapshot body-map entity {entity} has no primary record"
                )));
            };
            let frame = frames[frame_ordinal];
            if View::u32_le_at(bytes, frame.start) != Some(3)
                || bytes.get(frame.start + 4..frame.start + 7) != Some(class_tag.as_bytes())
                || View::u64_le_at(bytes, frame.start + 7) != Some(entity)
                || bytes.get(frame.start + 15..frame.start + 21) != Some(&[0; 6])
            {
                return Err(crate::error::malformed(format!(
                    "F3D Design snapshot body-map entity {entity} has an invalid entity header"
                )));
            }
            if let Some(record) =
                parse_snapshot_body_map_frame(ctx, bytes, meta, frame.start, frame.end, entity)?
            {
                ctx.charge_collection_items(1, "f3d snapshot body-map records")?;
                out.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d snapshot body-map records allocation", 0, 1)
                })?;
                out.push(record);
            }
        }
    }
    Ok(out)
}

fn parse_snapshot_body_map_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    start: usize,
    end: usize,
    entity: u64,
) -> Result<Option<BodyMapRecord>, CodecError> {
    let Some(companion_entity) = entity.checked_add(1) else {
        return Ok(None);
    };
    let Some(companion_at) = start.checked_add(21) else {
        return Ok(None);
    };
    for companion in local_reference_candidates(bytes, companion_at, true) {
        let reserved_count = 2 - companion.padding.trailing_zeros();
        let Some(reserved_end) = companion.end.checked_add(reserved_count) else {
            continue;
        };
        if companion.target != companion_entity
            || !reference_has_type(
                meta,
                &companion,
                crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
            )
            || !bytes
                .get(companion.end..reserved_end)
                .is_some_and(|reserved| reserved.iter().all(|byte| *byte == 0))
        {
            continue;
        }
        let count_at = reserved_end;
        let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
            continue;
        };
        let count = usize::try_from(pair_count)
            .map_err(|_| crate::error::malformed("F3D snapshot body-map count exceeds usize"))?;
        let Some(pairs_start) = count_at.checked_add(4) else {
            continue;
        };
        let Some(pairs_end) = count
            .checked_mul(16)
            .and_then(|span| pairs_start.checked_add(span))
        else {
            continue;
        };
        let Some(pairs) = bytes.get(pairs_start..pairs_end) else {
            continue;
        };
        if pairs.chunks_exact(16).any(|pair| {
            View::u64_le_at(pair, 8).is_none_or(|body_entity| {
                !entity_has_type(
                    meta,
                    body_entity,
                    crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                )
            })
        }) {
            continue;
        }
        for container in local_reference_candidates(bytes, pairs_end, false) {
            let reserved_count = 3 - container.padding.trailing_zeros();
            let Some(reserved_end) = container.end.checked_add(reserved_count) else {
                continue;
            };
            if !reference_has_type(
                meta,
                &container,
                crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            ) || !bytes
                .get(container.end..reserved_end)
                .is_some_and(|reserved| reserved.iter().all(|byte| *byte == 0))
            {
                continue;
            }
            let name_at = reserved_end;
            let Some(max_chars) = name_at
                .checked_add(4)
                .and_then(|payload| end.checked_sub(payload))
                .map(|remaining| remaining / 2)
            else {
                continue;
            };
            let Some((blob_name, name_end)) = lp_utf16_bounded_charged(ctx, bytes, name_at, 0..=max_chars)?
            else {
                continue;
            };
            if name_end != end
                || (!blob_name.is_empty()
                    && (!is_brep_blob_basename(&blob_name)
                        || std::path::Path::new(&blob_name).extension()
                            != Some(std::ffi::OsStr::new("smb"))))
                || (blob_name.is_empty() && pair_count != 0)
            {
                continue;
            }
            let mut bindings = Vec::new();
            ctx.charge_collection_items(u64::try_from(count).map_err(|_| {
                ctx.refuse_codec_limit("f3d snapshot body-map pair count", 0, 1)
            })?, "f3d snapshot body-map pairs")?;
            bindings.try_reserve(count).map_err(|_| {
                ctx.refuse_codec_limit("f3d snapshot body-map pairs allocation", 0, 1)
            })?;
            for (ordinal, pair) in pairs.chunks_exact(16).enumerate() {
                let mut pair = View::over_retained(pair);
                bindings.push(BodyBinding {
                    asm_key: pair.req_u64_le()?,
                    asm_key_offset: pairs_start + ordinal * 16,
                    entity_suffix: pair.req_u64_le()?,
                });
            }
            return Ok(Some(BodyMapRecord {
                blob_name,
                blob_name_offset: name_at + 4,
                bindings,
            }));
        }
    }
    Ok(None)
}

/// Parse every exactly indexed BREP body-map record in a Design `BulkStream`.
///
/// The type GUID names a family with more than one record frame. The
/// `MetaStream` entity list and primary record index select exact candidate
/// extents. A candidate is a body map only when one supported reserved-zero
/// width makes its count, pair run, tail, and basename consume that extent.
fn body_map_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyMapRecord>, CodecError> {
    let record_frames = crate::metastream::primary_record_frames(meta, bytes.len())?;

    let mut primary_by_entity = HashMap::<u64, Option<usize>>::new();
    for (ordinal, record) in meta.records.iter().enumerate() {
        if !primary_by_entity.contains_key(&record.entity_id) {
            ctx.charge_collection_items(1, "f3d body-map primary index")?;
            primary_by_entity.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d body-map primary index allocation", 0, 1)
            })?;
        }
        primary_by_entity
            .entry(record.entity_id)
            .and_modify(|record_ordinal| *record_ordinal = None)
            .or_insert(Some(ordinal));
    }

    let mut out = Vec::new();
    let mut typed_entities = HashSet::new();
    for (type_ordinal, design_type) in meta.types.iter().enumerate() {
        if !design_type
            .type_guid
            .as_str()
            .eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_TYPE_GUID)
        {
            continue;
        }
        if design_type.version != crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION {
            return Err(CodecError::NotImplemented(format!(
                "unsupported F3D Design body-map carrier version {}",
                design_type.version
            )));
        }
        if design_type.module != DESIGN_MODULE_BODY
            || !design_type
                .base_type_guid
                .value()
                .map(crate::records::mesh::DesignRelaxedGuidText::as_str)
                .is_some_and(|base| {
                    base.eq_ignore_ascii_case(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID)
                })
        {
            return Err(CodecError::Malformed(
                "F3D Design body-map carrier type has incompatible registration metadata".into(),
            ));
        }
        let class_tag = u32::try_from(type_ordinal)
            .ok()
            .and_then(|ordinal| ordinal.checked_add(256))
            .filter(|class_tag| *class_tag <= 999)
            .ok_or_else(|| {
                CodecError::Malformed(
                    "F3D Design body-map carrier class tag is not three digits".into(),
                )
            })?;
        let class_tag = class_tag.to_string();

        for &entity_id in design_type.entities.values() {
            if typed_entities.contains(&entity_id) {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design body-map carrier entity {entity_id} is registered more than once"
                )));
            }
            ctx.charge_collection_items(1, "f3d body-map typed entities")?;
            typed_entities.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d body-map typed entities allocation", 0, 1)
            })?;
            typed_entities.insert(entity_id);
            let record_ordinal = match primary_by_entity.get(&entity_id) {
                Some(Some(record_ordinal)) => *record_ordinal,
                Some(None) => {
                    return Err(CodecError::malformed(format_args!(
                        "F3D Design body-map carrier entity {entity_id} has multiple primary records"
                    )));
                }
                None => {
                    return Err(CodecError::malformed(format_args!(
                        "F3D Design body-map carrier entity {entity_id} has no primary record"
                    )));
                }
            };
            let frame = record_frames[record_ordinal];
            let start = frame.start;
            let end = frame.end;
            let record_index = u32::try_from(entity_id).map_err(|_| {
                CodecError::malformed(format_args!(
                    "F3D Design body-map carrier entity {entity_id} exceeds u32"
                ))
            })?;
            if View::u32_le_at(bytes, start) != Some(3)
                || bytes.get(
                    start + indexed_design_record_header::CLASS_TAG
                        ..start + indexed_design_record_header::RECORD_INDEX,
                ) != Some(class_tag.as_bytes())
                || View::u32_le_at(bytes, start + indexed_design_record_header::RECORD_INDEX)
                    != Some(record_index)
            {
                return Err(CodecError::malformed(format_args!(
                    "F3D Design body-map carrier entity {entity_id} has an invalid indexed header"
                )));
            }

            let mut matched = None;
            for prefix_len in crate::design::body::BODY_MAP_ZERO_PREFIX_LENGTHS {
                let Some(bindings) = parse_body_map_frame(ctx, bytes, meta, start, end, prefix_len)?
                else {
                    continue;
                };
                if matched.replace(bindings).is_some() {
                    return Err(CodecError::malformed(format_args!(
                        "F3D Design body-map carrier entity {entity_id} has an ambiguous frame"
                    )));
                }
            }
            if let Some(record) = matched {
                ctx.charge_collection_items(1, "f3d body-map records")?;
                out.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d body-map records allocation", 0, 1)
                })?;
                out.push(record);
            }
        }
    }
    Ok(out)
}

pub(crate) fn body_bindings(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyBinding>, CodecError> {
    let records = body_map_records(ctx, bytes, meta)?;
    let count = records.iter().try_fold(0usize, |total, record| {
        total.checked_add(record.bindings.len()).ok_or_else(|| {
            ctx.refuse_codec_limit("f3d flattened body-map pair count", 0, 1)
        })
    })?;
    ctx.charge_collection_items(u64::try_from(count).map_err(|_| {
        ctx.refuse_codec_limit("f3d flattened body-map pair count", 0, 1)
    })?, "f3d flattened body-map pairs")?;
    let mut bindings = Vec::new();
    bindings.try_reserve(count).map_err(|_| {
        ctx.refuse_codec_limit("f3d flattened body-map pairs allocation", 0, 1)
    })?;
    for record in records {
        bindings.extend(record.bindings);
    }
    Ok(bindings)
}

fn selected_body_map_records(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<Vec<BodyMapRecord>, CodecError> {
    let modern = body_map_records(ctx, bytes, meta)?;
    if modern.is_empty() {
        snapshot_body_map_records(ctx, bytes, meta)
    } else {
        Ok(modern)
    }
}

fn copy_body_map_name(
    ctx: &DecodeContext<'_>,
    name: &str,
    operation: &'static str,
) -> Result<String, CodecError> {
    String::from_utf8(ctx.copy_retained(name.as_bytes(), operation)?)
        .map_err(|_| CodecError::malformed("F3D body-map name must be UTF-8"))
}

/// Return the typed model-blob set selected independently in each Design
/// stream. The modern `.smbh` map takes precedence over snapshot `.smb` maps.
pub(crate) fn design_model_blob_names(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<Vec<String>, CodecError> {
    let mut model_names = Vec::new();
    let mut carrier_counts = HashMap::<String, usize>::new();
    let mut saw_design_stream = false;
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        saw_design_stream = true;
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let modern = body_map_records(ctx, bytes, &metadata)?;
        let snapshots = snapshot_body_map_records(ctx, bytes, &metadata)?;
        for record in modern.iter().chain(&snapshots) {
            if !record.blob_name.is_empty() {
                if let Some(count) = carrier_counts.get_mut(&record.blob_name) {
                    *count += 1;
                } else {
                    let name = copy_body_map_name(ctx, &record.blob_name, "f3d body-map carrier name")?;
                    ctx.charge_collection_items(1, "f3d body-map carrier counts")?;
                    carrier_counts.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("f3d body-map carrier counts allocation", 0, 1)
                    })?;
                    carrier_counts.insert(name, 1);
                }
            }
        }
        let selected = if modern.is_empty() {
            &snapshots
        } else {
            &modern
        };
        for record in selected.iter().filter(|record| !record.blob_name.is_empty()) {
            let name = copy_body_map_name(ctx, &record.blob_name, "f3d selected body-map name")?;
            ctx.charge_collection_items(1, "f3d selected body-map names")?;
            model_names.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d selected body-map names allocation", 0, 1)
            })?;
            model_names.push(name);
        }
    }

    let mut archive_counts = HashMap::<String, usize>::new();
    for entry in scan.entries.iter().filter(|entry| {
        scan.belongs_to_design_asset(&entry.name)
            && matches!(entry.role, ContainerRole::BrepSmb | ContainerRole::BrepSmbh)
    }) {
        let basename = entry.name.rsplit('/').next().unwrap_or(&entry.name);
        if let Some(count) = archive_counts.get_mut(basename) {
            *count += 1;
        } else {
            let name = copy_body_map_name(ctx, basename, "f3d archive BREP basename")?;
            ctx.charge_collection_items(1, "f3d archive BREP counts")?;
            archive_counts.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d archive BREP counts allocation", 0, 1)
            })?;
            archive_counts.insert(name, 1);
        }
    }
    if !saw_design_stream || carrier_counts.is_empty() {
        ctx.charge_collection_items(u64::try_from(archive_counts.len()).map_err(|_| {
            ctx.refuse_codec_limit("f3d archive BREP name count", 0, 1)
        })?, "f3d archive BREP names")?;
        let mut names = Vec::new();
        names.try_reserve(archive_counts.len()).map_err(|_| {
            ctx.refuse_codec_limit("f3d archive BREP names allocation", 0, 1)
        })?;
        names.extend(archive_counts.into_keys());
        names.sort();
        return Ok(names);
    }
    if carrier_counts != archive_counts {
        return Err(crate::error::malformed(
            "Design body-map carriers do not classify every binary BREP entry exactly once",
        ));
    }
    model_names.sort();
    model_names.dedup();
    Ok(model_names)
}

fn parse_body_map_frame(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
    start: usize,
    end: usize,
    prefix_len: usize,
) -> Result<Option<BodyMapRecord>, CodecError> {
    let Some(count_at) = start
        .checked_add(indexed_design_record_header::LEN)
        .and_then(|payload| payload.checked_add(prefix_len))
    else {
        return Ok(None);
    };
    if !bytes
        .get(start + indexed_design_record_header::LEN..count_at)
        .is_some_and(|prefix| prefix.iter().all(|byte| *byte == 0))
    {
        return Ok(None);
    }
    let Some(pair_count) = View::u32_le_at(bytes, count_at) else {
        return Ok(None);
    };
    let count = usize::try_from(pair_count).map_err(|_| {
        CodecError::malformed(format_args!(
            "F3D Design body map at byte {start} pair count does not fit this platform"
        ))
    })?;
    let Some(pairs_start) = count_at.checked_add(4) else {
        return Ok(None);
    };
    let Some(pairs_end) = count
        .checked_mul(16)
        .and_then(|span| pairs_start.checked_add(span))
    else {
        return Ok(None);
    };
    let decode_name = |name_at: usize| -> Result<Option<(usize, String)>, CodecError> {
        let Some(max_name_chars) = name_at
            .checked_add(4)
            .and_then(|payload| end.checked_sub(payload))
            .map(|remaining| remaining / 2) else {
            return Ok(None);
        };
        let Some((blob_name, name_end)) =
            lp_utf16_bounded_charged(ctx, bytes, name_at, 0..=max_name_chars)? else {
            return Ok(None);
        };
        Ok((name_end == end
            && ((pair_count == 0 && blob_name.is_empty())
                || (pair_count > 0
                    && is_brep_blob_basename(&blob_name)
                    && std::path::Path::new(&blob_name).extension()
                        == Some(std::ffi::OsStr::new("smbh")))))
        .then_some((name_at, blob_name)))
    };
    let mut typed_name = None;
    for reference in local_reference_candidates(bytes, pairs_end, true) {
        if !reference_has_type(
            meta,
            &reference,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
        ) {
            continue;
        }
        if let Some(name) = decode_name(reference.end)? {
            if typed_name.replace(name).is_some() {
                return Err(crate::error::malformed(
                    "F3D Design body-map frame has ambiguous typed reference tails",
                ));
            }
        }
    }
    let fixed_name = if typed_name.is_none()
        && View::u64_le_at(bytes, pairs_end).is_some()
        && View::u32_le_at(bytes, pairs_end + 8) == Some(0)
    {
        match pairs_end.checked_add(12) {
            Some(name_at) => decode_name(name_at)?,
            None => None,
        }
    } else {
        None
    };
    let Some((name_at, blob_name)) = typed_name.or(fixed_name) else {
        return Ok(None);
    };

    let mut bindings = Vec::new();
    ctx.charge_collection_items(u64::from(pair_count), "f3d body-map pairs")?;
    bindings.try_reserve(count).map_err(|_| {
        ctx.refuse_codec_limit("f3d body-map pairs allocation", 0, 1)
    })?;
    for pair in 0..count {
        let at = pairs_start + pair * 16;
        let (Some(key), Some(suffix)) =
            (View::u64_le_at(bytes, at), View::u64_le_at(bytes, at + 8))
        else {
            return Err(CodecError::malformed(format_args!(
                "F3D Design body map at byte {start} has a truncated pair run"
            )));
        };
        bindings.push(BodyBinding {
            asm_key: key,
            asm_key_offset: at,
            entity_suffix: suffix,
        });
    }
    Ok(Some(BodyMapRecord {
        blob_name,
        blob_name_offset: name_at + 4,
        bindings,
    }))
}

fn is_brep_blob_basename(value: &str) -> bool {
    let extension = value.rsplit_once('.').map(|(_, extension)| extension);
    value.starts_with("BREP.")
        && matches!(extension, Some("smb" | "smbh"))
        && !value.contains(['/', '\\'])
}

/// Decode every ordered Design BREP body-map pair and resolve each pair in its
/// named blob's body-selector namespace.
pub(crate) fn decode_design_body_bindings(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan,
    active_brep_entry: Option<&str>,
    body_keys: &[BodyNativeKey],
) -> Result<Vec<DesignBodyBinding>, CodecError> {
    let active_basename = active_brep_entry.and_then(|entry| entry.rsplit('/').next());
    let mut out = Vec::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        for record in selected_body_map_records(ctx, bytes, &metadata)? {
            let pair_count = u32::try_from(record.bindings.len())
                .map_err(|_| CodecError::malformed("F3D Design body map exceeds u32::MAX pairs"))?;
            let mut source_bodies = Vec::new();
            if pair_count != 0 {
                for key in body_keys.iter().filter(|key| {
                    key.source_brep.as_deref().map_or_else(
                        || active_basename == Some(record.blob_name.as_str()),
                        |source| source == record.blob_name,
                    )
                }) {
                    ctx.charge_collection_items(1, "f3d source BREP body keys")?;
                    source_bodies.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("f3d source BREP body keys allocation", 0, 1)
                    })?;
                    source_bodies.push(key);
                }
            }
            for (ordinal, binding) in (0..pair_count).zip(&record.bindings) {
                let body = crate::brep::resolve_body_selector(&source_bodies, binding.asm_key)?;
                let record = DesignBodyBinding::try_from(crate::records::bodies::DesignBodyBindingWire {
                        id: design_record_id_charged(
                            ctx,
                            &entry.name,
                            ":design-body-binding#",
                            u64_from_index(binding.asm_key_offset),
                            "f3d body record identifier",
                            "f3d body record identifier allocation",
                        )?,
                        stream: copy_body_map_name(ctx, &entry.name, "f3d body-binding stream")?,
                        pair_count,
                        pair_ordinal: ordinal,
                        asm_body_key: binding.asm_key,
                        asm_body_key_offset: binding.asm_key_offset as u64,
                        entity_suffix: binding.entity_suffix,
                        entity_suffix_offset: binding.entity_suffix_offset() as u64,
                        blob_name: copy_body_map_name(ctx, &record.blob_name, "f3d body-binding blob name")?,
                        blob_name_offset: record.blob_name_offset as u64,
                        body,
                    })
                    .map_err(CodecError::Malformed)?;
                ctx.charge_collection_items(1, "f3d decoded body bindings")?;
                out.try_reserve(1).map_err(|_| {
                    ctx.refuse_codec_limit("f3d decoded body bindings allocation", 0, 1)
                })?;
                out.push(record);
            }
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

/// Bind each body cache to every BREP map pair carrying the same Design entity
/// suffix in the same stream.
pub(crate) fn bind_body_bounds(
    ctx: &DecodeContext<'_>,
    bounds: &mut [DesignBodyBounds],
    bindings: &[DesignBodyBinding],
) -> Result<(), CodecError> {
    for bounds in bounds {
        let Some(stream) = native_stream(&bounds.id) else {
            continue;
        };
        let mut matches = Vec::new();
        for binding in bindings {
            if binding.entity_suffix != bounds.entity_suffix()
                || stream != native_scope_charged(ctx, &binding.stream)?
            {
                continue;
            }
            ctx.charge_collection_items(1, "f3d matching body bounds bindings")?;
            matches.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d matching body bounds bindings allocation", 0, 1)
            })?;
            matches.push(binding);
        }
        matches.sort_by_key(|binding| binding.asm_body_key_offset());
        let mut ids = Vec::new();
        for binding in matches {
            let id = copy_body_map_name(ctx, &binding.id, "f3d body bounds binding identifier")?;
            ctx.charge_collection_items(1, "f3d body bounds binding identifiers")?;
            ids.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d body bounds binding identifiers allocation", 0, 1)
            })?;
            ids.push(id);
        }
        bounds.body_binding_ids = ids;
    }
    Ok(())
}

/// Decode per-body display visibility from the Design `BulkStream`.
///
/// Each BREP body-map record resolves blob-qualified body selectors to Design
/// entity suffixes, and each entity's browser-node record carries a hidden flag
/// directly after the node GUID
/// ([spec §3.1](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/f3d.md#31-design-metadata)).
/// The result maps each blob and body selector to its display visibility;
/// bodies without records are absent.
#[derive(Debug, Clone)]
pub(crate) struct DecodedBodyVisibility {
    pub(crate) stream: String,
    pub(crate) byte_offset: u64,
    pub(crate) asm_body_key_offset: u64,
    pub(crate) entity_suffix: u64,
    pub(crate) visible: bool,
}

pub(crate) fn decode_all_body_visibility(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
) -> Result<HashMap<(String, u64), DecodedBodyVisibility>, CodecError> {
    let mut out = HashMap::new();
    for entry in scan
        .entries
        .iter()
        .filter(|entry| scan.is_design_stream(entry, ContainerRole::Bulkstream))
    {
        let bytes = scan.entry_bytes(&entry.name)?;
        let Some(metadata) =
            crate::design::decode::meta::metadata_for_bulk_stream(scan, &entry.name)?
        else {
            continue;
        };
        let hidden_by_entity = typed_browser_node_hidden_flags(ctx, bytes, &metadata)?;
        for record in selected_body_map_records(ctx, bytes, &metadata)? {
            for binding in record.bindings {
                let Some(node) = hidden_by_entity.get(&binding.entity_suffix) else {
                    continue;
                };
                ctx.charge_work(u64_from_index(out.len()), "f3d visibility duplicate lookup")?;
                let existing = out.keys().any(|(name, asm_key)| {
                    name == &record.blob_name && *asm_key == binding.asm_key
                });
                let key = copy_body_map_name(ctx, &record.blob_name, "f3d visibility BREP name")?;
                let stream = copy_body_map_name(ctx, &entry.name, "f3d visibility stream")?;
                if !existing {
                    ctx.charge_collection_items(1, "f3d body visibility entries")?;
                    out.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("f3d body visibility entries allocation", 0, 1)
                    })?;
                }
                out.insert(
                    (key, binding.asm_key),
                    DecodedBodyVisibility {
                        stream,
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

/// Visibility selected from one typed browser-node record.
#[derive(Debug, Clone, Copy)]
struct BrowserNodeVisibility {
    byte_offset: u64,
    hidden: bool,
}

fn typed_browser_node_hidden_flags(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    meta: &crate::metastream::MetaStream,
) -> Result<HashMap<u64, BrowserNodeVisibility>, CodecError> {
    let nodes = crate::design::decode::presentation::browser_node_records(ctx, bytes, meta)?;
    let presentations = crate::design::decode::presentation::body_presentations(ctx, bytes, meta)?;
    let mut nodes_by_entity = HashMap::<u64, Vec<_>>::new();
    for node in &nodes {
        if !nodes_by_entity.contains_key(&node.entity_suffix) {
            ctx.charge_collection_items(1, "f3d browser visibility entities")?;
            nodes_by_entity.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d browser visibility entities allocation", 0, 1)
            })?;
        }
        let candidates = nodes_by_entity.entry(node.entity_suffix).or_default();
        ctx.charge_collection_items(1, "f3d browser visibility candidates")?;
        candidates.try_reserve(1).map_err(|_| {
            ctx.refuse_codec_limit("f3d browser visibility candidates allocation", 0, 1)
        })?;
        candidates.push(node);
    }

    let mut out = HashMap::new();
    for (entity_suffix, candidates) in nodes_by_entity {
        let mut linked = Vec::new();
        for node in presentations
            .iter()
            .filter(|presentation| presentation.entity_suffix == entity_suffix)
            .filter_map(|presentation| presentation.browser_node.as_ref())
        {
            ctx.charge_collection_items(1, "f3d linked browser visibility nodes")?;
            linked.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d linked browser visibility nodes allocation", 0, 1)
            })?;
            linked.push(node);
        }
        linked.sort_by_key(|node| node.record_index);
        linked.dedup_by_key(|node| node.record_index);
        let selected = match linked.as_slice() {
            [node] => Some(*node),
            [] => match candidates.as_slice() {
                [node] => Some(*node),
                _ => None,
            },
            _ => None,
        };
        if let Some(node) = selected {
            ctx.charge_collection_items(1, "f3d selected browser visibility")?;
            out.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d selected browser visibility allocation", 0, 1)
            })?;
            out.insert(
                entity_suffix,
                BrowserNodeVisibility {
                    byte_offset: node.hidden_offset,
                    hidden: node.hidden,
                },
            );
        }
    }
    Ok(out)
}

/// Map each browser-node GUID to its Design entity suffix.
///
/// The GUID is the stable join between browser presentation records; the
/// adjacent entity suffix joins the node back to the Design body map.
pub(crate) fn scanned_browser_node_entities(
    ctx: &DecodeContext<'_>,
    bytes: &[u8],
) -> Result<HashMap<String, u64>, CodecError> {
    let mut entities = HashMap::new();
    let mut ambiguous = std::collections::HashSet::new();
    for record in scan_browser_node_identities(bytes) {
        let key = record.guid.to_ascii_lowercase();
        if let Some(previous) = entities.get_mut(&key) {
            if *previous != record.entity_suffix {
                *previous = record.entity_suffix;
                if !ambiguous.contains(&key) {
                    ctx.charge_collection_items(1, "f3d ambiguous browser GUIDs")?;
                    ambiguous.try_reserve(1).map_err(|_| {
                        ctx.refuse_codec_limit("f3d ambiguous browser GUIDs allocation", 0, 1)
                    })?;
                    ambiguous.insert(key);
                }
            }
        } else {
            ctx.charge_collection_items(1, "f3d scanned browser GUIDs")?;
            entities.try_reserve(1).map_err(|_| {
                ctx.refuse_codec_limit("f3d scanned browser GUIDs allocation", 0, 1)
            })?;
            entities.insert(key, record.entity_suffix);
        }
    }
    entities.retain(|guid, _| !ambiguous.contains(guid));
    Ok(entities)
}

#[derive(Debug)]
struct ScannedBrowserNodeIdentity {
    guid: String,
    entity_suffix: u64,
}

fn scan_browser_node_identities(bytes: &[u8]) -> impl Iterator<Item = ScannedBrowserNodeIdentity> + '_ {
    const GUID_CHARS: usize = 36;
    const GUID_BYTES: usize = GUID_CHARS * 2;
    let mut at = 0usize;
    std::iter::from_fn(move || {
        while at + 4 + GUID_BYTES + 3 + 8 <= bytes.len() {
            let candidate_at = at;
            at += 1;
            if View::u32_le_at(bytes, candidate_at) != Some(GUID_CHARS as u32)
                || !is_utf16_guid(&bytes[candidate_at + 4..candidate_at + 4 + GUID_BYTES])
            {
                continue;
            }
            let flag_at = candidate_at + 4 + GUID_BYTES;
            if bytes.get(flag_at + 1..flag_at + 3) == Some(&[0x01, 0x01]) {
                if let (0 | 1, Some(member)) = (bytes[flag_at], View::u64_le_at(bytes, flag_at + 3)) {
                    return Some(ScannedBrowserNodeIdentity {
                    guid: utf16_le_string(&bytes[candidate_at + 4..candidate_at + 4 + GUID_BYTES]),
                    entity_suffix: member,
                });
                }
            }
        }
        None
    })
}

fn utf16_le_string(bytes: &[u8]) -> String {
    let mut view = View::over_retained(bytes);
    let mut units = Vec::with_capacity(bytes.len() / 2);
    while let Some(unit) = view.u16_le() {
        units.push(unit);
    }
    String::from_utf16_lossy(&units)
}

fn is_utf16_guid(bytes: &[u8]) -> bool {
    bytes
        .chunks_exact(2)
        .all(|pair| pair[1] == 0 && (pair[0].is_ascii_hexdigit() || pair[0] == b'-'))
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use zip::CompressionMethod;

    use super::{
        body_bindings, body_bound_candidates, parse_body_map_frame, snapshot_body_map_records,
        typed_browser_node_hidden_flags,
    };
    use crate::bytes::lp_utf16_bytes;
    use crate::bytes::take_reference;
    use crate::design::presentation::{
        APPEARANCE_LIBRARY_ID, BODY_PRESENTATION_BASE_TYPE_GUID, BODY_PRESENTATION_TYPE_GUID,
        BODY_PRESENTATION_TYPE_VERSION, BODY_SCENE_NODE_TYPE_GUID, BODY_SCENE_NODE_TYPE_VERSION,
        BREP_CONTAINER_TYPE_GUID, BREP_CONTAINER_TYPE_VERSION, BROWSER_NODE_BASE_TYPE_GUID,
        BROWSER_NODE_TYPE_GUID, BROWSER_NODE_TYPE_VERSION, PHYSICAL_MATERIAL_LIBRARY_ID,
    };
    use crate::design::test_support::{design_type, primary_record};
    use crate::records::entity_header::DESIGN_MODULE_BODY;
    use crate::records::entity_header::DESIGN_MODULE_FUSION;
    use crate::test_support::indexed_header;
    use crate::test_support::manifest_test::write_synthetic_manifests;
    use crate::test_support::push_reference_u64;
    use crate::test_support::zip_test::with_scan;

    #[test]
    fn body_member_collections_and_identity_refuse_caller_limits() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut bulk = Vec::new();
        bulk.extend_from_slice(&10_u32.to_le_bytes());
        bulk.extend_from_slice(b"BodiesRoot");
        bulk.extend_from_slice(&0_u16.to_le_bytes());
        bulk.extend_from_slice(&10_u32.to_le_bytes());
        bulk.extend_from_slice(b"BodiesRoot");
        bulk.extend_from_slice(&1_u32.to_le_bytes());
        let member_offset = bulk.len();
        bulk.push(1);
        bulk.extend_from_slice(&9_u64.to_le_bytes());
        bulk.extend_from_slice(&2_u16.to_le_bytes());
        bulk.push(0);
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(&bulk).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            for (collection_limit, retained_limit, dimension, operation) in [
                (0, u64::MAX, ResourceDimension::CollectionItems, "f3d body members"),
                (1, u64::MAX, ResourceDimension::CollectionItems, "f3d decoded body members"),
                (1, 0, ResourceDimension::RetainedBytes, "f3d native stream key"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_collection_items = collection_limit;
                policy.limits.max_retained_bytes = retained_limit;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                assert!(matches!(
                    super::decode_body_members(&ctx, scan),
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ));
            }
            crate::design::test_support::with_test_decode_context(|ctx| {
                let members = super::decode_body_members(ctx, scan).unwrap();
                assert_eq!(members.len(), 1);
                assert_eq!(members[0].id, crate::ids::native_design_body_member_id(ENTRY, member_offset));
            });
        });
    }

    fn push_entity_header(out: &mut Vec<u8>, class_tag: &str, entity: u64) {
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(class_tag.as_bytes());
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend(lp_utf16_bytes(&format!("0_{entity}")));
    }

    fn body_map_bytes(prefix_len: usize, declared_count: u32, pairs: &[(u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        indexed_header(&mut out, *b"256", 900);
        out.extend(std::iter::repeat_n(0, prefix_len));
        out.extend_from_slice(&declared_count.to_le_bytes());
        for (key, suffix) in pairs {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        out.extend_from_slice(&1793u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend(lp_utf16_bytes(if declared_count == 0 {
            ""
        } else {
            "BREP.synthetic.smbh"
        }));
        out
    }

    fn body_map_bytes_with_typed_tail(prefix_len: usize, pairs: &[(u64, u64)]) -> Vec<u8> {
        let mut out = Vec::new();
        indexed_header(&mut out, *b"256", 900);
        out.extend(std::iter::repeat_n(0, prefix_len));
        out.extend_from_slice(&(pairs.len() as u32).to_le_bytes());
        for (key, suffix) in pairs {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&suffix.to_le_bytes());
        }
        push_snapshot_reference(
            &mut out,
            700,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            1,
        );
        out.push(0);
        out.extend(lp_utf16_bytes("BREP.synthetic.smbh"));
        out
    }

    fn body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![
                crate::records::entity_header::SegmentType {
                    id: String::new(),
                    byte_offset: 0,
                    type_guid: crate::design::body::BODY_MAP_CARRIER_TYPE_GUID
                        .to_owned()
                        .try_into()
                        .expect("type GUID"),
                    type_guid_offset: 0,
                    base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                        value: crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID
                            .to_owned()
                            .try_into()
                            .expect("base GUID"),
                        offset: 0,
                    },
                    version: crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                    version_offset: 0,
                    module: DESIGN_MODULE_BODY.into(),
                    entities: crate::records::identity::ReferenceRun::located(vec![
                        crate::records::identity::Located {
                            value: 900,
                            offset: 0,
                        },
                    ]),
                },
                design_type(
                    crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![700],
                ),
            ],
            records: vec![crate::metastream::RecordIndexEntry {
                entity_id: 900,
                bulk_offset: 0,
            }],
            secondary_records: Vec::new(),
        }
    }

    fn push_snapshot_reference(out: &mut Vec<u8>, target: u64, target_type: &str, form: u8) {
        match form {
            1 => {
                out.push(1);
                out.extend_from_slice(&target.to_le_bytes());
                out.extend_from_slice(&(target_type.len() as u32).to_le_bytes());
                out.extend_from_slice(target_type.as_bytes());
                out.extend_from_slice(&[0, 0]);
            }
            2 => {
                out.extend_from_slice(&[1, 1]);
                out.extend_from_slice(&target.to_le_bytes());
            }
            // Form 0 and every other stated form write the plain reference.
            _ => push_reference_u64(out, target),
        }
    }

    fn snapshot_body_map_bytes_with(
        form: u8,
        pair_count: u32,
        blob_name: &str,
        companion_type: &str,
    ) -> Vec<u8> {
        let entity = 900u64;
        let mut out = Vec::new();
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(b"256");
        out.extend_from_slice(&entity.to_le_bytes());
        out.extend_from_slice(&[0; 6]);
        push_snapshot_reference(&mut out, entity + 1, companion_type, form);
        if form == 2 {
            out.extend_from_slice(&[0, 0]);
        }
        out.extend_from_slice(&pair_count.to_le_bytes());
        if pair_count != 0 {
            out.extend_from_slice(&7u64.to_le_bytes());
            out.extend_from_slice(&500u64.to_le_bytes());
        }
        push_snapshot_reference(
            &mut out,
            700,
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
            form,
        );
        if form == 2 {
            out.extend_from_slice(&[0, 0, 0]);
        } else {
            out.push(0);
        }
        out.extend(lp_utf16_bytes(blob_name));
        out
    }

    fn snapshot_body_map_bytes(form: u8) -> Vec<u8> {
        snapshot_body_map_bytes_with(
            form,
            1,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        )
    }

    fn snapshot_body_map_metadata() -> crate::metastream::MetaStream {
        crate::metastream::MetaStream {
            types: vec![
                design_type(
                    crate::design::body::SNAPSHOT_BODY_MAP_CARRIER_TYPE_GUID,
                    Some(crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID),
                    1,
                    DESIGN_MODULE_BODY,
                    vec![900],
                ),
                design_type(
                    crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![901],
                ),
                design_type(
                    crate::design::presentation::BODY_PRESENTATION_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![500],
                ),
                design_type(
                    crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
                    None,
                    0,
                    DESIGN_MODULE_BODY,
                    vec![700],
                ),
            ],
            records: vec![primary_record(900, 0)],
            secondary_records: Vec::new(),
        }
    }

    #[test]
    fn snapshot_body_map_refuses_name_and_collection_limits() {
        let bytes = snapshot_body_map_bytes(0);
        let metadata = snapshot_body_map_metadata();
        for (items, retained, dimension, operation) in [
            (0, u64::MAX, ResourceDimension::CollectionItems, "f3d snapshot body-map primary index"),
            (1, u64::MAX, ResourceDimension::CollectionItems, "f3d snapshot body-map pairs"),
            (2, u64::MAX, ResourceDimension::CollectionItems, "f3d snapshot body-map records"),
            (u64::MAX, 0, ResourceDimension::RetainedBytes, "f3d Design UTF-16 text"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                snapshot_body_map_records(&ctx, &bytes, &metadata),
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == dimension && failure.operation == operation
            ));
        }
    }

    #[test]
    fn snapshot_body_map_accepts_every_reference_envelope() {
        for form in 0..=2 {
            let records = snapshot_body_map_records(&cadmpeg_test_support::service_decode_context(),
                &snapshot_body_map_bytes(form),
                &snapshot_body_map_metadata(),
            )
            .expect("typed snapshot body map");
            assert_eq!(records.len(), 1);
            assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
            assert_eq!(records[0].bindings.len(), 1);
            assert_eq!(records[0].bindings[0].asm_key, 7);
            assert_eq!(records[0].bindings[0].entity_suffix, 500);
        }
    }

    #[test]
    fn snapshot_body_map_accepts_three_zero_companion_variant() {
        let mut bytes = snapshot_body_map_bytes(0);
        let mut companion_end = 4 + 3 + 8 + 6;
        take_reference(&bytes, &mut companion_end).expect("ordinary companion reference");
        bytes.insert(companion_end, 0);

        let records = snapshot_body_map_records(&cadmpeg_test_support::service_decode_context(), &bytes, &snapshot_body_map_metadata())
            .expect("three-zero companion variant");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert_eq!(records[0].bindings.len(), 1);
    }

    #[test]
    fn snapshot_body_map_accepts_padded_doubled_companion() {
        let mut bytes = snapshot_body_map_bytes(2);
        let companion_end = 4 + 3 + 8 + 6 + 2 + 8;
        bytes.insert(companion_end, 0);

        let records = snapshot_body_map_records(&cadmpeg_test_support::service_decode_context(), &bytes, &snapshot_body_map_metadata())
            .expect("padded doubled companion");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert_eq!(records[0].bindings.len(), 1);
    }

    #[test]
    fn snapshot_body_map_requires_typed_pair_targets() {
        let mut metadata = snapshot_body_map_metadata();
        metadata.types[2].entities = crate::records::identity::ReferenceRun::unlocated(Vec::new());
        assert!(
            snapshot_body_map_records(&cadmpeg_test_support::service_decode_context(), &snapshot_body_map_bytes(0), &metadata)
                .expect("mixed carrier family")
                .is_empty()
        );
    }

    #[test]
    fn snapshot_body_map_retains_named_zero_pair_blob() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "BREP.snapshot.smb",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let records = snapshot_body_map_records(&cadmpeg_test_support::service_decode_context(), &bytes, &snapshot_body_map_metadata())
            .expect("named zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].blob_name, "BREP.snapshot.smb");
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_accepts_empty_zero_pair_record() {
        let bytes = snapshot_body_map_bytes_with(
            0,
            0,
            "",
            crate::design::body::SNAPSHOT_BODY_LIST_TYPE_GUID,
        );
        let records = snapshot_body_map_records(&cadmpeg_test_support::service_decode_context(), &bytes, &snapshot_body_map_metadata())
            .expect("empty zero-pair snapshot body map");
        assert_eq!(records.len(), 1);
        assert!(records[0].blob_name.is_empty());
        assert!(records[0].bindings.is_empty());
    }

    #[test]
    fn snapshot_body_map_rejects_contradictory_inline_type() {
        let bytes = snapshot_body_map_bytes_with(
            1,
            1,
            "BREP.snapshot.smb",
            crate::design::presentation::BREP_CONTAINER_TYPE_GUID,
        );
        assert!(
            snapshot_body_map_records(&cadmpeg_test_support::service_decode_context(), &bytes, &snapshot_body_map_metadata())
                .expect("mixed carrier family")
                .is_empty()
        );
    }

    #[test]
    fn body_map_count_is_bounded_by_the_stream_not_sixty_four_pairs() {
        let pairs = (0u64..65)
            .map(|ordinal| (1000 + ordinal, (1u64 << 40) + ordinal))
            .collect::<Vec<_>>();
        let bindings = body_bindings(&cadmpeg_test_support::service_decode_context(), &body_map_bytes(10, 65, &pairs), &body_map_metadata())
            .expect("65-pair body map");
        assert_eq!(bindings.len(), 65);
        assert_eq!(bindings[0].asm_key, 1000);
        assert_eq!(bindings[64].asm_key, 1064);
        assert_eq!(bindings[64].entity_suffix, (1u64 << 40) + 64);
    }

    #[test]
    fn modern_body_map_refuses_name_and_collection_limits() {
        let bytes = body_map_bytes(10, 1, &[(7, 500)]);
        let metadata = body_map_metadata();
        for (items, retained, dimension, operation) in [
            (0, u64::MAX, ResourceDimension::CollectionItems, "f3d body-map primary index"),
            (1, u64::MAX, ResourceDimension::CollectionItems, "f3d body-map typed entities"),
            (2, u64::MAX, ResourceDimension::CollectionItems, "f3d body-map pairs"),
            (3, u64::MAX, ResourceDimension::CollectionItems, "f3d body-map records"),
            (4, u64::MAX, ResourceDimension::CollectionItems, "f3d flattened body-map pairs"),
            (u64::MAX, 0, ResourceDimension::RetainedBytes, "f3d Design UTF-16 text"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                body_bindings(&ctx, &bytes, &metadata),
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == dimension && failure.operation == operation
            ));
        }
    }

    #[test]
    fn body_map_accepts_typed_container_reference_tail() {
        let bytes = body_map_bytes_with_typed_tail(10, &[(2291, 7492), (2292, 7534)]);
        let bindings = body_bindings(&cadmpeg_test_support::service_decode_context(), &bytes, &body_map_metadata()).expect("typed reference tail");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].asm_key, 2291);
        assert_eq!(bindings[0].entity_suffix, 7492);
        assert_eq!(bindings[1].asm_key, 2292);
        assert_eq!(bindings[1].entity_suffix, 7534);
    }

    #[test]
    fn both_empty_body_map_prefixes_have_no_pairs_or_brep_basename() {
        for prefix_len in crate::design::body::BODY_MAP_ZERO_PREFIX_LENGTHS {
            let bytes = body_map_bytes(prefix_len, 0, &[]);
            let frame =
                parse_body_map_frame(&cadmpeg_test_support::service_decode_context(), &bytes, &body_map_metadata(), 0, bytes.len(), prefix_len)
                    .expect("empty body-map frame")
                    .expect("supported empty body-map variant");
            assert!(frame.bindings.is_empty());
            assert!(body_bindings(&cadmpeg_test_support::service_decode_context(), &bytes, &body_map_metadata())
                .expect("empty typed body map")
                .is_empty());
        }
    }

    #[test]
    fn body_map_header_prevents_a_high_word_count_alias() {
        let bytes = body_map_bytes(10, 2, &[(10, (1u64 << 32) + 77), (20, 30)]);
        let bindings =
            body_bindings(&cadmpeg_test_support::service_decode_context(), &bytes, &body_map_metadata()).expect("typed two-pair body map");
        assert_eq!(bindings.len(), 2);
        assert_eq!(bindings[0].entity_suffix, (1u64 << 32) + 77);
        assert_eq!(bindings[1].asm_key, 20);
    }

    #[test]
    fn truncated_body_map_frame_is_not_decoded() {
        let bytes = body_map_bytes(10, 2, &[(10, 20)]);
        assert!(body_bindings(&cadmpeg_test_support::service_decode_context(), &bytes, &body_map_metadata())
            .expect("typed carrier record")
            .is_empty());
    }

    #[test]
    fn body_map_parser_does_not_scan_an_unindexed_nested_header() {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"256", 900);
        bytes.extend_from_slice(&[0xff; 4]);
        bytes.extend(body_map_bytes(10, 1, &[(10, 20)]));

        assert!(body_bindings(&cadmpeg_test_support::service_decode_context(), &bytes, &body_map_metadata())
            .expect("outer typed carrier record")
            .is_empty());
    }

    #[test]
    fn composed_body_map_outputs_refuse_caller_limits() {
        const PREFIX: &str = "FusionAssetName[Active]/Design1/";
        const BREP: &str = "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh";
        let mut bulk = body_map_bytes(10, 1, &[(7, 42)]);
        let browser_offset = bulk.len();
        push_browser_node(
            &mut bulk,
            100,
            "11111111-2222-8333-A444-555555555555",
            false,
            42,
        );
        let metadata = crate::test_support::streams_test::design_metastream_with_records(
            &[
                (
                    crate::design::body::BODY_MAP_CARRIER_TYPE_GUID,
                    crate::design::body::BODY_MAP_CARRIER_BASE_TYPE_GUID,
                    crate::design::body::BODY_MAP_CARRIER_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    &[900],
                ),
                (
                    BROWSER_NODE_TYPE_GUID,
                    BROWSER_NODE_BASE_TYPE_GUID,
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    &[100],
                ),
            ],
            &[(900, 0), (100, browser_offset as u64)],
        );
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(format!("{PREFIX}BulkStream.dat"), stored).unwrap();
        zip.write_all(&bulk).unwrap();
        zip.start_file(format!("{PREFIX}MetaStream.dat"), stored).unwrap();
        zip.write_all(&metadata).unwrap();
        zip.start_file(BREP, stored).unwrap();
        zip.write_all(&[0]).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        with_scan(&archive, |scan| {
            let visibility = super::decode_all_body_visibility(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            ).unwrap();
            assert!(visibility.get(&("BREP.synthetic.smbh".to_owned(), 7)).unwrap().visible);
            let names = super::design_model_blob_names(
                &cadmpeg_test_support::service_decode_context(),
                scan,
            ).unwrap();
            assert_eq!(names, ["BREP.synthetic.smbh"]);
            let bindings = super::decode_design_body_bindings(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                None,
                &[],
            ).unwrap();
            assert_eq!(bindings.len(), 1);
            let body_key = cadmpeg_asm::brep::records::BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body: "f3d:brep:entity#7".try_into().unwrap(),
                record_index: 7,
                body_ordinal: 0,
                source_brep: Some("BREP.synthetic.smbh".into()),
                asm_body_key: Some(7),
            };
            let blob_len = "BREP.synthetic.smbh".len() as u64;
            let stream_name = format!("{PREFIX}BulkStream.dat");
            let scope_len = crate::ids::native_scope(&stream_name).len() as u64;
            let suffix_len = format!(
                ":design-body-binding#{}",
                bindings[0].asm_body_key_offset(),
            ).len() as u64;
            for (items, operation) in [
                (30, "f3d body visibility entries"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_collection_items = items;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::decode_all_body_visibility(&ctx, scan);
                assert!(matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::CollectionItems
                            && failure.operation == operation
                ), "item limit {items}: {result:?}");
            }
            for (items, operation) in [
                (7, "f3d body-map carrier counts"),
                (8, "f3d selected body-map names"),
                (9, "f3d archive BREP counts"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_collection_items = items;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::design_model_blob_names(&ctx, scan);
                assert!(matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::CollectionItems
                            && failure.operation == operation
                ), "blob-name item limit {items}: {result:?}");
            }
            for (retained, operation) in [
                (blob_len, "f3d body-map carrier name"),
                (blob_len * 2, "f3d selected body-map name"),
                (blob_len * 3, "f3d archive BREP basename"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = retained;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::design_model_blob_names(&ctx, scan);
                assert!(matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::RetainedBytes
                            && failure.operation == operation
                ), "blob-name retained limit {retained}: {result:?}");
            }
            for (items, operation) in [
                (5, "f3d source BREP body keys"),
                (6, "f3d decoded body bindings"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_collection_items = items;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::decode_design_body_bindings(&ctx, scan, None, &[body_key.clone()]);
                assert!(matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::CollectionItems
                            && failure.operation == operation
                ), "binding item limit {items}: {result:?}");
            }
            for (retained, operation) in [
                (blob_len, "f3d native stream key"),
                (blob_len + scope_len, "f3d body record identifier"),
                (blob_len + scope_len + suffix_len, "f3d body-binding stream"),
                (blob_len + scope_len + suffix_len + stream_name.len() as u64,
                    "f3d body-binding blob name"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = retained;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::decode_design_body_bindings(&ctx, scan, None, &[body_key.clone()]);
                assert!(matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::RetainedBytes
                            && failure.operation == operation
                ), "binding retained limit {retained}: {result:?}");
            }
            for (retained, operation) in [
                (72 + blob_len, "f3d visibility BREP name"),
                (72 + blob_len * 2, "f3d visibility stream"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_retained_bytes = retained;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::decode_all_body_visibility(&ctx, scan);
                assert!(matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == ResourceDimension::RetainedBytes
                            && failure.operation == operation
                ), "visibility retained limit {retained}: {result:?}");
            }
        });
    }

    fn push_browser_node(
        out: &mut Vec<u8>,
        record_index: u32,
        guid: &str,
        hidden: bool,
        entity: u64,
    ) -> u64 {
        indexed_header(out, *b"257", record_index);
        out.extend_from_slice(&[0; 10]);
        out.extend(lp_utf16_bytes(guid));
        let hidden_offset = out.len() as u64;
        out.push(u8::from(hidden));
        out.extend_from_slice(&[1, 1]);
        out.extend_from_slice(&entity.to_le_bytes());
        hidden_offset
    }

    #[test]
    fn presentation_guid_selects_visibility_when_suffix_repeats() {
        let entity = 42u64;
        let selected_guid = "11111111-2222-8333-A444-555555555555";
        let competing_guid = "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_entity_header(&mut bytes, "256", entity);
        bytes.extend(lp_utf16_bytes(selected_guid));
        bytes.extend_from_slice(&[1, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        bytes.extend(lp_utf16_bytes("99999999-8888-8777-A666-555555555555"));
        bytes.extend(lp_utf16_bytes(PHYSICAL_MATERIAL_LIBRARY_ID));
        bytes.extend(lp_utf16_bytes("PrismMaterial-001"));
        push_reference_u64(&mut bytes, 7);
        bytes.push(0);
        push_reference_u64(&mut bytes, entity + 1);
        bytes.extend(lp_utf16_bytes("Body"));
        bytes.extend_from_slice(&1.0f32.to_le_bytes());
        bytes.extend_from_slice(&[1, 1]);
        bytes.extend(lp_utf16_bytes("12345678-1234-8234-A234-123456789ABC"));
        bytes.extend(lp_utf16_bytes(APPEARANCE_LIBRARY_ID));
        let selected_start = bytes.len();
        let selected_offset = push_browser_node(&mut bytes, 100, selected_guid, false, entity);
        let competing_start = bytes.len();
        push_browser_node(&mut bytes, 101, competing_guid, true, entity);

        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    vec![entity],
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
                design_type(
                    BREP_CONTAINER_TYPE_GUID,
                    None,
                    BREP_CONTAINER_TYPE_VERSION,
                    "",
                    vec![7],
                ),
                design_type(
                    BODY_SCENE_NODE_TYPE_GUID,
                    None,
                    BODY_SCENE_NODE_TYPE_VERSION,
                    "",
                    vec![entity + 1],
                ),
            ],
            records: vec![
                primary_record(entity, 0),
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let visibility = crate::design::test_support::with_test_decode_context(|ctx| {
            typed_browser_node_hidden_flags(ctx, &bytes, &meta)
        }).expect("typed presentation graph");
        let selected = visibility.get(&entity).expect("presentation-selected node");
        assert_eq!(selected.byte_offset, selected_offset);
        assert!(!selected.hidden);

        let mut nodes_only = Vec::new();
        let selected_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 100, selected_guid, false, entity);
        let competing_start = nodes_only.len();
        push_browser_node(&mut nodes_only, 101, competing_guid, true, entity);
        let meta = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    Vec::new(),
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100, 101],
                ),
            ],
            records: vec![
                primary_record(100, selected_start),
                primary_record(101, competing_start),
            ],
            secondary_records: Vec::new(),
        };
        let visibility = crate::design::test_support::with_test_decode_context(|ctx| {
            typed_browser_node_hidden_flags(ctx, &nodes_only, &meta)
        }).expect("typed browser nodes");
        assert!(
            !visibility.contains_key(&entity),
            "two unjoined typed nodes are ambiguous"
        );
    }

    #[test]
    fn browser_visibility_refuses_entity_candidate_and_output_limits() {
        let mut bytes = Vec::new();
        push_browser_node(
            &mut bytes,
            100,
            "11111111-2222-8333-A444-555555555555",
            true,
            42,
        );
        let metadata = crate::metastream::MetaStream {
            types: vec![
                design_type(
                    BODY_PRESENTATION_TYPE_GUID,
                    Some(BODY_PRESENTATION_BASE_TYPE_GUID),
                    BODY_PRESENTATION_TYPE_VERSION,
                    DESIGN_MODULE_BODY,
                    Vec::new(),
                ),
                design_type(
                    BROWSER_NODE_TYPE_GUID,
                    Some(BROWSER_NODE_BASE_TYPE_GUID),
                    BROWSER_NODE_TYPE_VERSION,
                    DESIGN_MODULE_FUSION,
                    vec![100],
                ),
            ],
            records: vec![primary_record(100, 0)],
            secondary_records: Vec::new(),
        };
        for (items, operation) in [
            (15, "f3d browser visibility entities"),
            (16, "f3d browser visibility candidates"),
            (17, "f3d selected browser visibility"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let result = typed_browser_node_hidden_flags(&ctx, &bytes, &metadata);
            assert!(matches!(
                &result,
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ), "item limit {items}: {result:?}");
        }
        let selected = typed_browser_node_hidden_flags(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
            &metadata,
        ).unwrap();
        assert!(selected.get(&42).unwrap().hidden);
    }

    #[test]
    fn scanned_browser_guids_refuse_map_and_ambiguity_limits() {
        const GUID: &str = "AAAAAAAA-BBBB-8CCC-9DDD-EEEEEEEEEEEE";
        let mut bytes = Vec::new();
        push_browser_node(&mut bytes, 100, GUID, false, 42);
        push_browser_node(&mut bytes, 101, GUID, true, 43);
        for (items, operation) in [
            (0, "f3d scanned browser GUIDs"),
            (1, "f3d ambiguous browser GUIDs"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            assert!(matches!(
                super::scanned_browser_node_entities(&ctx, &bytes),
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == ResourceDimension::CollectionItems
                        && failure.operation == operation
            ));
        }
        let entities = super::scanned_browser_node_entities(
            &cadmpeg_test_support::service_decode_context(),
            &bytes,
        ).unwrap();
        assert!(entities.is_empty());
    }

    #[test]
    fn body_bound_candidate_has_one_marker_and_six_ordered_f64_values() {
        let values: [f64; 6] = [4.0, 6.0, 1.5, -1.0, 0.0, -0.25];
        let mut bytes = vec![1];
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let candidates = body_bound_candidates(&bytes, 0, bytes.len()).collect::<Vec<_>>();
        assert_eq!(
            candidates
                .into_iter()
                .map(|(offset, values)| (offset, values.map(cadmpeg_ir::scalar::FiniteReal::get)))
                .collect::<Vec<_>>(),
            [(0, values)]
        );

        bytes[0] = 0;
        assert!(body_bound_candidates(&bytes, 0, bytes.len())
            .next()
            .is_none());
    }

    #[test]
    fn body_bounds_binding_refuses_matching_and_identifier_limits() {
        use crate::records::bodies::{
            DesignBodyBinding, DesignBodyBindingWire, DesignBodyBounds, DesignBodyBoundsWire,
        };
        const STREAM: &str = "Design/BulkStream.dat";
        let binding = DesignBodyBinding::try_from(DesignBodyBindingWire {
            id: "f3d:binding#1".into(),
            stream: STREAM.into(),
            pair_count: 1,
            pair_ordinal: 0,
            asm_body_key: 9,
            asm_body_key_offset: 20,
            entity_suffix: 7,
            entity_suffix_offset: 28,
            blob_name: "BREP.synthetic.smbh".into(),
            blob_name_offset: 40,
            body: None,
        }).unwrap();
        let make_bounds = || DesignBodyBounds::try_from(DesignBodyBoundsWire {
            id: format!("{}:design-body-bounds#0", crate::ids::native_scope(STREAM)),
            entity_suffix: 7,
            entity_byte_offset: 0,
            record_indices: [8, 9, 10],
            record_byte_offsets: [10, 20, 30],
            value_byte_offsets: [11, 21, 31],
            body_binding_ids: Vec::new(),
            maximum: cadmpeg_ir::math::Point3::new(1.0, 1.0, 1.0),
            minimum: cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
        }).unwrap();
        let scope_len = crate::ids::native_scope(STREAM).len() as u64;
        for (items, retained, dimension, operation) in [
            (0, u64::MAX, ResourceDimension::CollectionItems, "f3d matching body bounds bindings"),
            (1, u64::MAX, ResourceDimension::CollectionItems, "f3d body bounds binding identifiers"),
            (u64::MAX, 0, ResourceDimension::RetainedBytes, "f3d native stream key"),
            (u64::MAX, scope_len, ResourceDimension::RetainedBytes, "f3d body bounds binding identifier"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut bounds = [make_bounds()];
            assert!(matches!(
                super::bind_body_bounds(&ctx, &mut bounds, &[binding.clone()]),
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == dimension && failure.operation == operation
            ));
        }
        let mut bounds = [make_bounds()];
        super::bind_body_bounds(&cadmpeg_test_support::service_decode_context(), &mut bounds, &[binding]).unwrap();
        assert_eq!(bounds[0].body_binding_ids, ["f3d:binding#1"]);
    }

    #[test]
    fn decoded_body_bounds_refuse_output_and_identifier_limits() {
        const ENTRY: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let mut bulk = Vec::new();
        indexed_header(&mut bulk, *b"256", 7);
        bulk.extend_from_slice(&[0; 5]);
        for record_index in [8, 9, 10] {
            indexed_header(&mut bulk, *b"257", record_index);
            bulk.push(1);
            for value in [4.0_f64, 6.0, 1.5, -1.0, 0.0, -0.25] {
                bulk.extend_from_slice(&value.to_le_bytes());
            }
        }
        let entity = crate::records::entity_header::DesignEntityHeader {
            id: format!("{}:design-entity-header#0", crate::ids::native_scope(ENTRY)),
            byte_offset: 0,
            entity_id: crate::records::identity::DesignEntityId::try_from("0_7".to_owned()).unwrap(),
            class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            optional_slot_present: false,
            registration: crate::records::entity_header::DesignEntityRegistration::new(
                Some(DESIGN_MODULE_BODY.to_owned()),
                None,
                crate::records::identity::ReferenceRun::unlocated(Vec::new()),
            ).unwrap(),
        };
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored = crate::zip_write::file_options(CompressionMethod::Stored);
        write_synthetic_manifests(&mut zip, stored);
        zip.start_file(ENTRY, stored).unwrap();
        zip.write_all(&bulk).unwrap();
        let archive = zip.finish().unwrap().into_inner();
        let scope_len = crate::ids::native_scope(ENTRY).len() as u64;
        let suffix_len = ":design-body-bounds#0".len() as u64;
        with_scan(&archive, |scan| {
            for (items, retained, dimension, operation) in [
                (0, u64::MAX, ResourceDimension::CollectionItems, "f3d body bounds"),
                (u64::MAX, 0, ResourceDimension::RetainedBytes, "f3d native stream key"),
                (u64::MAX, scope_len + suffix_len - 1, ResourceDimension::RetainedBytes, "f3d body record identifier"),
            ] {
                let arena = DecodeArena::new();
                let mut policy = DecodePolicy::default();
                policy.limits.max_collection_items = items;
                policy.limits.max_retained_bytes = retained;
                let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
                let result = super::decode_body_bounds(&ctx, scan, &[entity.clone()]);
                assert!(matches!(
                    &result,
                    Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                        if failure.dimension == dimension && failure.operation == operation
                ), "item limit {items}, retained limit {retained}: {result:?}");
            }
            let bounds = super::decode_body_bounds(
                &cadmpeg_test_support::service_decode_context(),
                scan,
                &[entity.clone()],
            ).unwrap();
            assert_eq!(bounds.len(), 1);
            assert_eq!(bounds[0].entity_suffix(), 7);
        });
    }

    #[test]
    fn a_recipe_marker_before_the_record_index_word_states_no_record_index() {
        // The family marker opens at offset four, so the stream holds no index
        // word sixteen bytes before it.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"body_recipe_data");
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&cadmpeg_test_support::service_decode_context(), &bytes, "Design/BulkStream.dat", &mut recipes).unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(recipes[0].record_index, None);
        let wire = serde_json::to_value(&recipes[0]).expect("recipe wire");
        assert_eq!(wire.get("record_index"), None);
        assert_eq!(wire.get("record_index_offset"), None);
    }

    #[test]
    fn construction_recipe_refuses_counter_output_and_identifier_limits() {
        const STREAM: &str = "Design/BulkStream.dat";
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(b"body_recipe_data");
        let scope_len = crate::ids::native_scope(STREAM).len() as u64;
        let suffix_len = ":construction-recipe#4".len() as u64;
        for (items, retained, dimension, operation) in [
            (0, u64::MAX, ResourceDimension::CollectionItems, "f3d construction recipe counters"),
            (1, u64::MAX, ResourceDimension::CollectionItems, "f3d construction recipes"),
            (u64::MAX, 0, ResourceDimension::RetainedBytes, "f3d native stream key"),
            (u64::MAX, scope_len + suffix_len - 1, ResourceDimension::RetainedBytes, "f3d body record identifier"),
        ] {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::default();
            policy.limits.max_collection_items = items;
            policy.limits.max_retained_bytes = retained;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            let mut recipes = Vec::new();
            assert!(matches!(
                super::decode_stream(&ctx, &bytes, STREAM, &mut recipes),
                Err(cadmpeg_core::CodecError::ResourceLimit(failure))
                    if failure.dimension == dimension && failure.operation == operation
            ));
        }
    }

    #[test]
    fn bounded_face_record_identity_is_not_a_second_design_id() {
        let mut bytes = Vec::new();
        for _ in 0..2 {
            let mut prefix = [0u8; 27];
            prefix[11..15].copy_from_slice(&309i32.to_le_bytes());
            prefix[23..27].copy_from_slice(&24u32.to_le_bytes());
            bytes.extend_from_slice(&prefix);
            bytes.extend_from_slice(b"bounded_face_recipe_data");
            bytes.extend_from_slice(&(-1i64).to_le_bytes());
        }
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&cadmpeg_test_support::service_decode_context(), &bytes, "Design/BulkStream.dat", &mut recipes).unwrap();
        assert_eq!(recipes.len(), 2);
        assert!(recipes
            .iter()
            .all(|recipe| recipe.record_index.map(|index| index.value) == Some(309)));
        assert!(recipes.iter().all(|recipe| recipe.design.is_none()));
        assert_eq!(recipes[0].recipe_index, 0);
        assert_eq!(recipes[1].recipe_index, 1);

        let mut body = Vec::new();
        body.extend_from_slice(&4u32.to_le_bytes());
        body.extend_from_slice(b"2265");
        body.extend_from_slice(&3u32.to_le_bytes());
        body.extend_from_slice(&[0; 12]);
        body.extend_from_slice(&16u32.to_le_bytes());
        body.extend_from_slice(b"body_recipe_data");
        let mut recipes = Vec::new();
        crate::design::decode::body::decode_stream(&cadmpeg_test_support::service_decode_context(), &body, "Design/BulkStream.dat", &mut recipes).unwrap();
        assert_eq!(recipes.len(), 1);
        assert_eq!(
            recipes[0]
                .design
                .as_ref()
                .map(|design| design.id.value.as_str()),
            Some("2265")
        );
        assert_eq!(
            recipes[0].design.as_ref().map(|design| design.id.offset),
            Some(4)
        );
        assert_eq!(
            recipes[0]
                .design
                .as_ref()
                .and_then(|design| design.selector),
            Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 3,
                byte_offset: 8,
            })
        );
    }
}
