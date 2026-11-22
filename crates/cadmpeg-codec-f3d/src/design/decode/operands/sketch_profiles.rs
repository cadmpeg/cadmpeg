// SPDX-License-Identifier: Apache-2.0
//! Sketch profile frames, regions, and profile curve selections.

use super::construction_groups::is_following_empty_extrude_record;
use super::extrude_selections::utf16_decimal_u64;
use super::face_sources::indexed_operand_headers;
use super::operand_candidates::sketch_profile_text_picks;
use crate::bytes::{lp_ascii_filtered_view, lp_utf16_bounded, take_reference};
use crate::container::ContainerScan;
use crate::design::decode::scopes::shared_frames::marked_record_reference;
use crate::design::decode::sketch::{
    indexed_record_header_at, next_indexed_record_offset, IndexedRecordOffsets,
};
use crate::design::decode::text::{design_record_id, relaxed_guid_end};
use crate::design::{design_feature_family, DesignFeatureFamily};
use crate::ids::native_stream;
use crate::layout::{
    compact_reference_profile_chain_record as compact_profile_chain,
    compact_reference_profile_identity_record as compact_profile_identity,
    compact_reference_profile_selection_prefix as compact_profile_selection,
    indexed_design_record_header as indexed_header,
    legacy_extrude_sketch_profile_tail as legacy_profile_tail,
    sketch_profile_region_member as region_member,
    sketch_profile_region_selection_prefix as region_selection,
    typed_reference_profile_chain_record as typed_chain,
    typed_reference_profile_identity_record as typed_identity,
    typed_reference_profile_selection_prefix as typed_selection,
};
use crate::records::decal::DesignRecordHeader;
use crate::records::entity_header::DesignEntityHeader;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignExtrudeOperandRole;
use crate::records::topology::sketch_profile::{
    DesignSketchProfileAtomicRegion, DesignSketchProfileOperand, DesignSketchProfileRegion,
    DesignSketchProfileRegionMember, DesignSketchProfileRegionSelection,
};
use cadmpeg_core::container::ContainerRole;
use cadmpeg_core::decode::View;
use cadmpeg_core::CodecError;
use cadmpeg_ir::report::loss::LossNote;
use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};

pub(crate) fn unresolved_profile_group_loss(
    profile_count: usize,
) -> cadmpeg_ir::report::loss::LossNote {
    crate::loss::F3dLossCode::FeatureExtrudeProfileGroupNative.note(format!(
        "{profile_count} Sketch-profile operands remain native because their group composition is unresolved.",
    ))
}

/// Several Sketch-profile operands of one Extrude are one selection when they
/// name the same Sketch and each carries a region selection.
pub(crate) fn composable_sketch_profiles(profiles: &[DesignSketchProfileOperand]) -> bool {
    let Some(first) = profiles.first() else {
        return false;
    };
    profiles.len() > 1
        && profiles.iter().all(|profile| {
            profile.entity_id == first.entity_id && profile.region_selection.is_some()
        })
}

/// Resolve the unique sketch-profile frame named by profile-based scopes, and
/// record the Extrudes whose operands carry text selections.
pub(crate) fn bind_sketch_profiles(
    scan: &ContainerScan,
    scopes: &mut [DesignParameterScope],
    headers: &[DesignRecordHeader],
    entities: &[DesignEntityHeader],
    text_extrude_candidates: &mut Vec<
        crate::design::sketch_text_extrude::SketchTextExtrudeCandidate,
    >,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, CodecError> {
    let headers = indexed_operand_headers(headers);
    let mut losses = Vec::new();
    let mut record_offsets = HashMap::<_, OnceCell<_>>::new();
    for scope in scopes.iter_mut().filter(|scope| {
        design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
            || design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sweep)
            || scope.emboss_operand_construction().is_some()
            || scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFlange
    }) {
        let Some(stream) = native_stream(&scope.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        let mut profiles = Vec::new();
        for (ordinal, record_index) in scope.reference_members().values().copied().enumerate() {
            if scope.emboss_operand_construction().is_some() && ordinal != 1 {
                continue;
            }
            let (Ok(ordinal), Some(header)) =
                (u32::try_from(ordinal), headers.get(&(stream, record_index)))
            else {
                continue;
            };
            let profile =
                parse_sketch_profile(bytes, stream, ordinal, header, entities).or_else(|| {
                    if scope.legacy_class_330_profile_member_ordinal() != Some(ordinal) {
                        return None;
                    }
                    parse_sketch_profile_frame(
                        bytes,
                        stream,
                        ordinal,
                        header,
                        entities,
                        Some((
                            scope.record_index,
                            record_offsets.entry(entry.name.as_str()).or_default(),
                        )),
                    )
                });
            let Some(profile) = profile else {
                continue;
            };
            profiles.push(profile);
        }
        let is_extrude = design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude);
        if profiles.iter().any(|profile| profile.sketch_absent) {
            if is_extrude {
                let scope_record_index = scope.record_index;
                if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
                    scope.payload_mut()
                {
                    let absent = profiles
                        .iter()
                        .filter(|profile| profile.sketch_absent)
                        .count();
                    losses.push(crate::loss::F3dLossCode::FeatureExtrudeProfileSketchAbsent.note(
                        format!("{absent} Sketch-profile operand(s) of Extrude scope record {scope_record_index} name a Sketch that the Design stream does not register; the profile stays native."),
                    ));
                    slot.get_or_insert_with(Default::default)
                        .extrude_native_profiles = Some(profiles);
                    continue;
                }
            } else {
                profiles.retain(|profile| !profile.sketch_absent);
            }
        }
        if profiles.len() > 1 {
            let scope_id = scope.id.clone();
            if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
                scope.payload_mut()
            {
                match sketch_profile_text_picks(bytes, &profiles) {
                    Some(picks) => text_extrude_candidates.push(
                        crate::design::sketch_text_extrude::SketchTextExtrudeCandidate {
                            scope_id,
                            picks,
                        },
                    ),
                    None if !composable_sketch_profiles(&profiles) => {
                        losses.push(unresolved_profile_group_loss(profiles.len()));
                    }
                    None => {}
                }
                slot.get_or_insert_with(Default::default)
                    .extrude_native_profiles = Some(profiles);
            }
            continue;
        }
        let Some(profile) = profiles.pop() else {
            continue;
        };
        if is_extrude {
            if let Some(picks) = sketch_profile_text_picks(bytes, std::slice::from_ref(&profile)) {
                text_extrude_candidates.push(
                    crate::design::sketch_text_extrude::SketchTextExtrudeCandidate {
                        scope_id: scope.id.clone(),
                        picks,
                    },
                );
            }
        }
        losses.extend(profile.flags_assumption_loss());
        if scope.kind() == crate::records::feature::scope::DesignFeatureKind::BaseFlange {
            if let crate::records::feature::scope::DesignScopePayloadMut::BaseFlange(slot) =
                scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default)
                    .base_flange_profile = Some(profile);
            }
        } else if scope.kind() == crate::records::feature::scope::DesignFeatureKind::Emboss {
            if profile.scope_reference_ordinal == 1 {
                if let crate::records::feature::scope::DesignScopePayloadMut::Emboss(Some(
                    construction,
                )) = scope.payload_mut()
                {
                    construction.sketch_profile = Some(profile);
                }
            }
        } else if design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Sweep) {
            if let crate::records::feature::scope::DesignScopePayloadMut::Sweep(slot) =
                scope.payload_mut()
            {
                slot.get_or_insert_with(Default::default).sweep_profile = Some(profile);
            }
        } else if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot) =
            scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_profile = Some(profile);
        }
    }
    Ok(losses)
}

pub(in crate::design) fn parse_sketch_profile(
    bytes: &[u8],
    stream: &str,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    entities: &[DesignEntityHeader],
) -> Option<DesignSketchProfileOperand> {
    parse_sketch_profile_frame(
        bytes,
        stream,
        scope_reference_ordinal,
        header,
        entities,
        None,
    )
}

pub(super) fn parse_sketch_profile_frame(
    bytes: &[u8],
    stream: &str,
    scope_reference_ordinal: u32,
    header: &DesignRecordHeader,
    entities: &[DesignEntityHeader],
    legacy: Option<(u32, &OnceCell<IndexedRecordOffsets>)>,
) -> Option<DesignSketchProfileOperand> {
    if legacy.is_some() && header.class_tag.as_str() != "272" {
        return None;
    }
    let start = usize::try_from(header.byte_offset).ok()?;
    let prefix_record_delta = if legacy.is_some() { 2 } else { 3 };
    if bytes.get(start + 11..start + 21)? != [0; 10]
        || bytes.get(start + 21) != Some(&1)
        || View::u32_le_at(bytes, start + 22)?
            != header.record_index.checked_add(prefix_record_delta)?
        || bytes.get(start + 26..start + 32)? != [0; 6]
        || View::u32_le_at(bytes, start + 32)? != 1
    {
        return None;
    }
    relaxed_guid_end(bytes, start + 36)?;
    let (asset_id, after_asset_id) = lp_utf16_bounded(bytes, start + 36, 1..=256)?;
    let (entity_suffix, after_entity_suffix) = utf16_decimal_u64(bytes, after_asset_id)?;
    let paired_at = next_indexed_record_offset(bytes, start + 11)?;
    let (paired_class_tag, after_paired_tag) =
        lp_ascii_filtered_view(bytes, paired_at, 0..=2000, u8::is_ascii_graphic)?;
    let tail_length = paired_at.checked_sub(after_entity_suffix)?;
    if View::u32_le_at(bytes, after_paired_tag)? != header.record_index
        || !matches!(tail_length, 82 | 83 | 89 | 93 | 94)
        || (legacy.is_some()
            && (tail_length != legacy_profile_tail::LEN || paired_class_tag != "262"))
    {
        return None;
    }
    if tail_length == 83 {
        // The 83-byte tail omits the marked reference that follows the third
        // u32 of the 94-byte tail, which is then zero. Its second u32 is
        // zero or one.
        let tail = after_entity_suffix;
        if View::u32_le_at(bytes, tail)? != 1
            || !matches!(View::u32_le_at(bytes, tail + 4)?, 0 | 1)
            || bytes.get(tail + 8..tail + 16)? != [0; 8]
            || View::u32_le_at(bytes, tail + 16)? != 0x41
            || marked_record_reference(bytes, tail + 46) != header.record_index.checked_add(2)
            || marked_record_reference(bytes, tail + 60) != header.record_index.checked_add(1)
            || bytes.get(tail + 71) != Some(&0)
            || marked_record_reference(bytes, tail + 72).is_none()
        {
            return None;
        }
    }
    let mut compact_tail_flags = None;
    if tail_length == 82 {
        // The 82-byte tail is the 83-byte tail with the nested-one reference
        // one byte earlier: two flag bytes that are zero or one precede it.
        let tail = after_entity_suffix;
        if View::u32_le_at(bytes, tail)? != 1
            || !matches!(View::u32_le_at(bytes, tail + 4)?, 0 | 1)
            || bytes.get(tail + 8..tail + 16)? != [0; 8]
            || View::u32_le_at(bytes, tail + 16)? != 0x41
            || marked_record_reference(bytes, tail + 46) != header.record_index.checked_add(2)
            || !bytes
                .get(tail + 57..tail + 59)?
                .iter()
                .all(|&flag| flag <= 1)
            || marked_record_reference(bytes, tail + 59) != header.record_index.checked_add(1)
            || bytes.get(tail + 70) != Some(&0)
            || marked_record_reference(bytes, tail + 71).is_none()
        {
            return None;
        }
        compact_tail_flags = Some([bytes[tail + 57] != 0, bytes[tail + 58] != 0]);
    }
    if matches!(tail_length, 89 | 93) {
        let tail = after_entity_suffix;
        let (nested_two_at, nested_one_at, scope_at) = if tail_length == 89 {
            (53, 66, 78)
        } else {
            (legacy_profile_tail::EMPTY_RECORD_REFERENCE, 70, 82)
        };
        let first_tail_target = marked_record_reference(bytes, tail + nested_two_at)?;
        let scope_target = marked_record_reference(bytes, tail + scope_at)?;
        if bytes.get(tail..tail + 8) != Some(&[1, 0, 0, 0, 0, 0, 0, 0])
            || View::u32_le_at(bytes, tail + 8) != Some(1)
            || !bytes
                .get(tail + nested_one_at - 2..tail + nested_one_at)?
                .iter()
                .all(|&flag| flag <= 1)
            || marked_record_reference(bytes, tail + nested_one_at)
                != header.record_index.checked_add(1)
            || bytes.get(tail + scope_at - 1) != Some(&0)
            || View::u32_le_at(bytes, tail + 41) == Some(0)
            || (tail_length == 93
                && View::u32_le_at(bytes, tail + 41) != View::u32_le_at(bytes, tail + 53))
        {
            return None;
        }
        if let Some((owner, offsets)) = legacy {
            if scope_target != owner
                || first_tail_target <= header.record_index
                || View::u32_le_at(bytes, tail + legacy_profile_tail::ROLE)
                    != Some(legacy_profile_tail::ROLE_VALUE)
                || bytes.get(tail + legacy_profile_tail::ZERO_FLAGS..tail + nested_one_at)
                    != Some(&[0, 0])
                || !is_following_empty_extrude_record(
                    bytes,
                    offsets,
                    first_tail_target,
                    after_paired_tag + size_of::<u32>(),
                )
            {
                return None;
            }
        } else if Some(first_tail_target) != header.record_index.checked_add(2) {
            return None;
        }
        compact_tail_flags = Some([
            bytes[tail + nested_one_at - 2] != 0,
            bytes[tail + nested_one_at - 1] != 0,
        ]);
    }
    let mut matches = entities.iter().filter(|entity| {
        native_stream(&entity.id) == Some(stream)
            && entity.in_sketch_module()
            && entity.entity_id.suffix() == entity_suffix
    });
    let (entity_id_text, sketch_absent) = match matches.next() {
        Some(entity) => {
            if matches.next().is_some() {
                return None;
            }
            (entity.entity_id.as_str().to_owned(), false)
        }
        None => {
            // No Sketch entity carries the suffix. The profile keeps the
            // stream's one Sketch entity prefix with the stored suffix.
            let mut prefixes = entities
                .iter()
                .filter(|entity| native_stream(&entity.id) == Some(stream))
                .filter(|entity| entity.in_sketch_module())
                .map(|entity| {
                    entity
                        .entity_id
                        .as_str()
                        .rsplit_once('_')
                        .map(|(prefix, _)| prefix)
                });
            let prefix = prefixes.next()??;
            if prefixes.any(|other| other != Some(prefix)) {
                return None;
            }
            (format!("{prefix}_{entity_suffix}"), true)
        }
    };
    let region_selection = parse_sketch_profile_region_selection_frame(
        bytes,
        header.record_index,
        paired_at,
        prefix_record_delta,
    );
    if legacy.is_some() && region_selection.is_none() {
        return None;
    }
    DesignSketchProfileOperand::try_new(
        crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
            sketch_absent,
            scope_reference_ordinal,
            record_index: header.record_index,
            byte_offset: header.byte_offset,
            class_tag: header.class_tag.clone(),
            asset_id: asset_id.try_into().ok()?,
            asset_id_offset: u64::try_from(start + 40).ok()?,
            entity_id: crate::records::identity::DesignEntityId::try_from(entity_id_text).ok()?,
            entity_reference_offset: u64::try_from(after_asset_id + 4).ok()?,
            compact_tail_flags,
            region_selection,
            paired_class_tag: crate::design::decode::text::class_tag_from_view(paired_class_tag)
                .ok()?,
            paired_byte_offset: u64::try_from(paired_at).ok()?,
        },
    )
    .ok()
}

pub(super) fn parse_sketch_profile_region_selection(
    bytes: &[u8],
    profile_record_index: u32,
    paired_at: usize,
) -> Option<DesignSketchProfileRegionSelection> {
    parse_sketch_profile_region_selection_frame(bytes, profile_record_index, paired_at, 3)
}

pub(super) fn parse_sketch_profile_region_selection_frame(
    bytes: &[u8],
    profile_record_index: u32,
    paired_at: usize,
    selection_delta: u32,
) -> Option<DesignSketchProfileRegionSelection> {
    const COUNT_LEN: usize = 4;
    // Each counted region stores at least its four-byte loop count, so the
    // bytes after a count bound it.
    let fits = |count: u32, after: usize| {
        usize::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(COUNT_LEN))
            .zip(bytes.len().checked_sub(after))
            .is_some_and(|(needed, left)| needed <= left)
    };

    let next_header = |position, expected| {
        let at = next_indexed_record_offset(bytes, position)?;
        (indexed_record_header_at(bytes, at)?.record_index == expected).then_some(at)
    };
    let mut predecessor = paired_at;
    for delta in 1..selection_delta {
        predecessor = next_header(
            predecessor.checked_add(indexed_header::LEN)?,
            profile_record_index.checked_add(delta)?,
        )?;
    }
    let selection_record_index = profile_record_index.checked_add(selection_delta)?;
    let selection_at = next_header(
        predecessor.checked_add(indexed_header::LEN)?,
        selection_record_index,
    )?;
    let (class_tag, after_class_tag) =
        lp_ascii_filtered_view(bytes, selection_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_class_tag)? != selection_record_index
        || bytes.get(
            selection_at + region_selection::ZERO_RUN_10
                ..selection_at + region_selection::PROFILE_REFERENCE_MARKER,
        )? != [0; 10]
        || marked_record_reference(
            bytes,
            selection_at + region_selection::PROFILE_REFERENCE_MARKER,
        ) != Some(profile_record_index)
        || bytes.get(
            selection_at + region_selection::ZERO_RUN_6
                ..selection_at + region_selection::REGION_COUNT,
        )? != [0; 6]
    {
        return None;
    }
    if selection_at.checked_add(region_selection::LEN)? > bytes.len() {
        return None;
    }
    let region_count = View::u32_le_at(bytes, selection_at + region_selection::REGION_COUNT)?;
    let mut cursor = selection_at.checked_add(region_selection::LOOP_COUNT)?;
    if region_count == 0 || !fits(region_count, cursor) {
        return None;
    }
    let mut regions = Vec::new();
    let mut atomic_regions = Vec::new();
    for region_ordinal in 0..region_count {
        let (loops, next) = parse_sketch_profile_region_loops(bytes, cursor, region_ordinal)?;
        regions.extend(loops);
        let atomic_count = View::u32_le_at(bytes, next)?;
        cursor = next.checked_add(COUNT_LEN)?;
        if !fits(atomic_count, cursor) {
            return None;
        }
        for _ in 0..atomic_count {
            let (loops, next) = parse_sketch_profile_region_loops(bytes, cursor, region_ordinal)?;
            atomic_regions.push(DesignSketchProfileAtomicRegion {
                region_ordinal,
                loop_count_offset: u64::try_from(cursor).ok()?,
                loops,
            });
            cursor = next;
        }
    }
    let companion_at = cursor;
    if next_header(companion_at, selection_record_index)? != companion_at {
        return None;
    }
    let (companion_class_tag, after_companion_class_tag) =
        lp_ascii_filtered_view(bytes, companion_at, 0..=2000, u8::is_ascii_graphic)?;
    if View::u32_le_at(bytes, after_companion_class_tag)? != selection_record_index {
        return None;
    }
    Some(DesignSketchProfileRegionSelection {
        record_index: selection_record_index,
        byte_offset: u64::try_from(selection_at).ok()?,
        class_tag: crate::design::decode::text::class_tag_from_view(class_tag).ok()?,
        region_count_offset: u64::try_from(selection_at.checked_add(region_selection::LOOP_COUNT)?)
            .ok()?,
        regions,
        atomic_regions,
        companion_class_tag: crate::design::decode::text::class_tag_from_view(companion_class_tag)
            .ok()?,
        companion_byte_offset: u64::try_from(companion_at).ok()?,
    })
}

/// The boundary loops of one selected or atomic region, starting at its
/// nonzero u32 loop count. Each loop ends with an exterior byte. Returns the
/// loops and the offset after the last exterior byte.
pub(super) fn parse_sketch_profile_region_loops(
    bytes: &[u8],
    at: usize,
    region_ordinal: u32,
) -> Option<(Vec<DesignSketchProfileRegion>, usize)> {
    const COUNT_LEN: usize = 4;
    const EXTERIOR_LEN: usize = 1;
    let loop_count = View::u32_le_at(bytes, at)?;
    // Each loop holds at least a count, one member, and its exterior byte.
    let minimum_loop_len = COUNT_LEN
        .checked_add(region_member::LEN)?
        .checked_add(EXTERIOR_LEN)?;
    let mut cursor = at.checked_add(COUNT_LEN)?;
    if loop_count == 0
        || usize::try_from(loop_count)
            .ok()?
            .checked_mul(minimum_loop_len)?
            .checked_add(cursor)?
            > bytes.len()
    {
        return None;
    }
    let mut loops = Vec::new();
    for _ in 0..loop_count {
        let (mut ring, next) = parse_sketch_profile_region_members(bytes, cursor, EXTERIOR_LEN)?;
        let &exterior = bytes.get(next).filter(|byte| matches!(byte, 0 | 1))?;
        ring.region_ordinal = region_ordinal;
        ring.hole = exterior == 0;
        loops.push(ring);
        cursor = next.checked_add(EXTERIOR_LEN)?;
    }
    Some((loops, cursor))
}

/// One counted member list of a nested sketch-profile selection, starting
/// at its u32 member count. `trailing_minimum_len` bytes must follow the
/// list. Returns the list and the offset after its last member.
pub(super) fn parse_sketch_profile_region_members(
    bytes: &[u8],
    mut cursor: usize,
    trailing_minimum_len: usize,
) -> Option<(DesignSketchProfileRegion, usize)> {
    const REGION_COUNT_LEN: usize = 4;
    let member_count_offset = cursor;
    let member_count = usize::try_from(View::u32_le_at(bytes, cursor)?).ok()?;
    cursor = cursor.checked_add(REGION_COUNT_LEN)?;
    if member_count == 0 {
        return None;
    }
    let member_run_len = member_count.checked_mul(region_member::LEN)?;
    if cursor
        .checked_add(member_run_len)?
        .checked_add(trailing_minimum_len)?
        > bytes.len()
    {
        return None;
    }
    let mut members = Vec::new();
    for _ in 0..member_count {
        let kind_offset = cursor;
        let kind = View::u32_le_at(bytes, cursor)?;
        let curve_primary_id = std::num::NonZeroU32::new(View::u32_le_at(
            bytes,
            cursor.checked_add(region_member::CURVE_PRIMARY_ID)?,
        )?)?;
        let incidence_words_offset = cursor.checked_add(region_member::ZERO_WORD_AFTER_PRIMARY)?;
        let mut incidence_words = [0; 8];
        for (ordinal, word) in incidence_words.iter_mut().enumerate() {
            *word = View::u32_le_at(
                bytes,
                incidence_words_offset.checked_add(ordinal.checked_mul(4)?)?,
            )?;
        }
        let curve_secondary_id = View::u32_le_at(
            bytes,
            cursor.checked_add(region_member::CURVE_SECONDARY_ID)?,
        )?;
        let zero_after_secondary = View::u32_le_at(
            bytes,
            cursor.checked_add(region_member::ZERO_WORD_AFTER_SECONDARY)?,
        )?;
        if kind != 3
            || incidence_words[0] != 0
            || zero_after_secondary != 0
            || !matches!(incidence_words[3], 0 | 1)
            || incidence_words[6..] != [0; 2]
        {
            return None;
        }
        members.push(DesignSketchProfileRegionMember {
            kind_offset: u64::try_from(kind_offset).ok()?,
            curve_primary_id,
            curve_primary_id_offset: u64::try_from(
                cursor.checked_add(region_member::CURVE_PRIMARY_ID)?,
            )
            .ok()?,
            curve_secondary_id: std::num::NonZeroU32::new(curve_secondary_id),
            incidence_flag: incidence_words[3] == 1,
            incidence_values: [
                crate::records::topology::sketch_profile::DesignRegionIncidence::try_from(
                    incidence_words[4],
                )
                .ok()?,
                crate::records::topology::sketch_profile::DesignRegionIncidence::try_from(
                    incidence_words[5],
                )
                .ok()?,
            ],
            incidence_words_offset: u64::try_from(incidence_words_offset).ok()?,
        });
        cursor = cursor.checked_add(region_member::LEN)?;
    }
    Some((
        DesignSketchProfileRegion {
            member_count_offset: u64::try_from(member_count_offset).ok()?,
            members,
            region_ordinal: 0,
            hole: false,
        },
        cursor,
    ))
}

/// Chain-record bytes after its fourteen zero bytes and before the typed
/// Sketch scope reference.
pub(super) const TYPED_PROFILE_CHAIN_MARKER: [u8; 6] = [1, 1, 1, 0, 0, 0];

pub(crate) fn decode_profile_curve_selections(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
) -> Result<Vec<crate::records::topology::entity_selection::DesignProfileCurveSelection>, CodecError>
{
    let headers = indexed_operand_headers(headers);
    let mut records_by_stream = HashMap::new();
    let mut out = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                continue;
            };
            let Some(header) = headers.get(&(stream, record_index)) else {
                continue;
            };
            let Ok(start) = usize::try_from(header.byte_offset) else {
                continue;
            };
            let mut reference_end = start + typed_selection::IDENTITY_REFERENCE;
            if take_reference(bytes, &mut reference_end)
                .and_then(|reference| reference.local().map(|(target, _)| target))
                != record_index.checked_add(3).map(u64::from)
            {
                continue;
            }
            let records = records_by_stream
                .entry(stream)
                .or_insert_with(|| IndexedRecordOffsets::build(bytes));
            let Some(mut selection) =
                parse_profile_curve_selection(ctx, bytes, records, group, ordinal, header)?
            else {
                continue;
            };
            selection.id = design_record_id(
                &entry.name,
                ":design-profile-curve-selection#",
                header.byte_offset,
            );
            out.push(selection);
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

pub(super) fn parse_profile_curve_selection_prefix(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    group_member_ordinal: u32,
    header: &DesignRecordHeader,
) -> Option<(
    crate::records::topology::entity_selection::DesignProfileCurveSelection,
    usize,
    usize,
    u32,
)> {
    use crate::records::topology::entity_selection::DesignProfileCurveSelection;
    let Ok(start) = usize::try_from(header.byte_offset) else {
        return None;
    };
    let record_index = header.record_index;
    let (Some(scope_index), Some(identity_index)) =
        (record_index.checked_add(1), record_index.checked_add(3))
    else {
        return None;
    };
    let mut after_reference = start + compact_profile_selection::IDENTITY_REFERENCE;
    let reference = take_reference(bytes, &mut after_reference)?;
    let (target, inline_type) = reference.local()?;
    let typed = inline_type.is_some();
    let (
        selection_zero,
        selection_reference,
        constant_one,
        asset_at,
        chain_zero,
        chain_marker,
        sketch_reference,
        identity_zero,
        selection_back_reference,
        group_count,
    ) = if typed {
        (
            typed_selection::ZERO_RUN_10,
            typed_selection::IDENTITY_REFERENCE,
            typed_selection::CONSTANT_ONE,
            typed_selection::LEN,
            typed_chain::ZERO_RUN_14,
            typed_chain::CHAIN_MARKER,
            typed_chain::SKETCH_SCOPE_REFERENCE,
            typed_identity::ZERO_RUN_10,
            typed_identity::SELECTION_REFERENCE,
            typed_identity::REGION_GROUP_COUNT,
        )
    } else {
        (
            compact_profile_selection::ZERO_RUN_10,
            compact_profile_selection::IDENTITY_REFERENCE,
            compact_profile_selection::CONSTANT_ONE,
            compact_profile_selection::LEN,
            compact_profile_chain::ZERO_RUN_14,
            compact_profile_chain::CHAIN_MARKER,
            compact_profile_chain::SKETCH_SCOPE_REFERENCE,
            compact_profile_identity::ZERO_RUN_10,
            compact_profile_identity::SELECTION_REFERENCE,
            compact_profile_identity::REGION_GROUP_COUNT,
        )
    };
    let reference_target = |at| {
        let mut end = at;
        let reference = take_reference(bytes, &mut end)?;
        let (target, inline_type) = reference.local()?;
        (inline_type.is_some() == typed).then_some((u32::try_from(target).ok()?, end))
    };
    if bytes.get(start + selection_zero..start + selection_reference) != Some(&[0; 10][..])
        || target != u64::from(identity_index)
        || after_reference != start + constant_one
        || View::u32_le_at(bytes, start + constant_one) != Some(1)
    {
        return None;
    }
    let (_, after_asset) = lp_utf16_bounded(bytes, start + asset_at, 36..=36)?;
    let (owner_text, _) = lp_utf16_bounded(bytes, after_asset, 1..=20)?;
    if !owner_text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let &scope_at = records.offsets(scope_index).first()?;
    if bytes.get(scope_at + chain_zero..scope_at + chain_marker) != Some(&[0; 14][..])
        || bytes.get(scope_at + chain_marker..scope_at + sketch_reference)
            != Some(&TYPED_PROFILE_CHAIN_MARKER[..])
    {
        return None;
    }
    let (sketch_scope_record_index, _) = reference_target(scope_at + sketch_reference)?;
    let scope_sketch_containers = match records.offsets(sketch_scope_record_index).first() {
        Some(&at) if typed => typed_sketch_container_references(bytes, at),
        _ => Vec::new(),
    };
    let &identity_at = records.offsets(identity_index).first()?;
    let (identity_target, groups_start) = reference_target(identity_at + selection_back_reference)?;
    if bytes.get(identity_at + identity_zero..identity_at + selection_back_reference)
        != Some(&[0; 10][..])
        || identity_target != record_index
        || groups_start != identity_at + group_count
    {
        return None;
    }
    let identity_end = next_indexed_record_offset(bytes, identity_at + identity_zero)?;
    if !typed && indexed_record_header_at(bytes, identity_end)?.record_index != identity_index {
        return None;
    }
    Some((
        DesignProfileCurveSelection {
            id: String::new(),
            scope_record_index: group.scope_record_index,
            group_record_index: group.record_index,
            group_member_ordinal,
            record_index,
            byte_offset: header.byte_offset,
            owner_text_offset: cadmpeg_core::decode::u64_from_index(after_asset + 4),
            owner_text,
            sketch_scope_record_index,
            sketch_scope_offset: cadmpeg_core::decode::u64_from_index(
                scope_at + sketch_reference + 1,
            ),
            scope_sketch_containers,
            identity_record_offset: cadmpeg_core::decode::u64_from_index(identity_at),
            identity_end_offset: cadmpeg_core::decode::u64_from_index(identity_end),
            groups: Vec::new(),
        },
        identity_end,
        groups_start,
        if typed { 2 } else { 3 },
    ))
}

/// The record indices that typed local references of type
/// [`crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID`] name inside the frame of the record at
/// `record_at`, in stored order without repeats. The frame ends at the next
/// indexed record header.
pub(super) fn typed_sketch_container_references(bytes: &[u8], record_at: usize) -> Vec<u32> {
    use crate::bytes::{typed_local_reference, TYPED_LOCAL_REFERENCE_LEN};
    let mut containers = Vec::new();
    let Some(end) = next_indexed_record_offset(bytes, record_at + indexed_header::LEN) else {
        return containers;
    };
    let mut at = record_at + indexed_header::LEN;
    while at + TYPED_LOCAL_REFERENCE_LEN <= end {
        if bytes[at] == 1 {
            if let Some((target, guid)) = typed_local_reference(bytes, at) {
                if guid
                    .eq_ignore_ascii_case(crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID)
                {
                    if !containers.contains(&target) {
                        containers.push(target);
                    }
                    at += TYPED_LOCAL_REFERENCE_LEN;
                    continue;
                }
            }
        }
        at += 1;
    }
    containers
}

pub(super) fn parse_profile_curve_selection(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    group: &DesignConstructionOperandGroup,
    ordinal: u32,
    header: &DesignRecordHeader,
) -> Result<
    Option<crate::records::topology::entity_selection::DesignProfileCurveSelection>,
    CodecError,
> {
    let Some((mut selection, end, groups_start, entry_kind)) =
        parse_profile_curve_selection_prefix(bytes, records, group, ordinal, header)
    else {
        return Ok(None);
    };
    if entry_kind == 3 {
        let Some(paired_at) = usize::try_from(selection.byte_offset)
            .ok()
            .and_then(|start| start.checked_add(indexed_header::LEN))
            .and_then(|start| next_indexed_record_offset(bytes, start))
        else {
            return Ok(None);
        };
        // A nested region selection whose members name curves without a
        // secondary identity is the sketch-profile frame's own selection.
        if parse_sketch_profile_region_selection(bytes, header.record_index, paired_at)
            .is_some_and(|selection| !selection.has_secondary_identity())
        {
            return Ok(None);
        }
    }
    let Some(groups) = parse_profile_region_groups(ctx, bytes, groups_start, end, entry_kind)?
    else {
        return Ok(None);
    };
    selection.groups = groups;
    Ok(selection.valid_layout().then_some(selection))
}

/// Smallest stored region: a loop count, an entry count, one entry and the
/// exterior flag.
pub(super) const MIN_PROFILE_REGION_BYTES: usize = 49;

pub(super) fn parse_profile_region(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    view: &mut View<'_>,
    entry_kind: u32,
) -> Result<
    Option<Vec<crate::records::topology::entity_selection::DesignProfileCurveLoop>>,
    CodecError,
> {
    use crate::records::topology::entity_selection::{
        DesignProfileCurveEntry, DesignProfileCurveLoop,
    };
    macro_rules! field {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let count = field!(view.u32_le());
    let count = field!(view.counted(u64::from(count), 45)).get();
    if count == 0 {
        return Ok(None);
    }
    let mut loops = ctx.alloc_filled(
        count,
        DesignProfileCurveLoop::default(),
        "F3D profile loops",
    )?;
    for loop_ in &mut loops {
        let count = field!(view.u32_le());
        let count = field!(view.counted(u64::from(count), 40)).get();
        if count == 0 {
            return Ok(None);
        }
        loop_.entries = ctx.alloc_filled(
            count,
            DesignProfileCurveEntry::default(),
            "F3D profile curve entries",
        )?;
        for entry in &mut loop_.entries {
            if view.u32_le() != Some(entry_kind) {
                return Ok(None);
            }
            let primary_id_offset = cadmpeg_core::decode::u64_from_index(view.position());
            *entry = DesignProfileCurveEntry {
                primary_id: field!(view.u64_le()),
                primary_id_offset,
                secondary_id: field!(view.u64_le()),
                entry_fields: [
                    field!(view.u32_le()),
                    field!(view.u32_le()),
                    field!(view.u32_le()),
                ],
            };
            if view.take(8) != Some(&[0; 8][..]) {
                return Ok(None);
            }
        }
        loop_.outer = match view.u8() {
            Some(0) => false,
            Some(1) => true,
            _ => return Ok(None),
        };
    }
    Ok(Some(loops))
}

pub(super) fn parse_profile_region_groups(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    start: usize,
    end: usize,
    entry_kind: u32,
) -> Result<
    Option<Vec<crate::records::topology::entity_selection::DesignProfileRegionGroup>>,
    CodecError,
> {
    use crate::records::topology::entity_selection::DesignProfileRegionGroup;
    macro_rules! field {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }
    let mut view = field!(View::over_retained(bytes).child(start, end));
    let count = field!(view.u32_le());
    // A group is one region and the u32 count of its further regions.
    let count = field!(view.counted(u64::from(count), MIN_PROFILE_REGION_BYTES + 4)).get();
    if count == 0 {
        return Ok(None);
    }
    let mut groups = ctx.alloc_filled(
        count,
        DesignProfileRegionGroup::default(),
        "F3D profile region groups",
    )?;
    for group in &mut groups {
        group.region = field!(parse_profile_region(ctx, &mut view, entry_kind)?);
        let count = field!(view.u32_le());
        let count = field!(view.counted(u64::from(count), MIN_PROFILE_REGION_BYTES)).get();
        group.further = ctx.alloc_filled(count, Vec::new(), "F3D profile regions")?;
        for region in &mut group.further {
            *region = field!(parse_profile_region(ctx, &mut view, entry_kind)?);
        }
    }
    Ok((view.remaining() == 0).then_some(groups))
}

/// Chain-record bytes after its fourteen zero bytes and before the typed
/// reference back to the selection record of a sketchless chain record.
pub(super) const SKETCHLESS_PROFILE_CHAIN_MARKER: [u8; 6] = [1, 1, 0, 0, 0, 0];

/// The asset GUID of the typed-reference or compact profile selection at
/// `header` when its chain record is sketchless.
pub(super) fn sketchless_profile_selection_asset(
    bytes: &[u8],
    records: &IndexedRecordOffsets,
    header: &DesignRecordHeader,
) -> Option<String> {
    use crate::layout::compact_reference_sketchless_profile_chain_record as compact_chain;
    use crate::layout::typed_reference_sketchless_profile_chain_record as typed_chain;
    let start = usize::try_from(header.byte_offset).ok()?;
    let record_index = header.record_index;
    let chain_index = record_index.checked_add(1)?;
    let identity_index = record_index.checked_add(3)?;
    let local_target = |at: usize| {
        let mut end = at;
        let reference = take_reference(bytes, &mut end)?;
        let (target, inline_type) = reference.local()?;
        Some((target, inline_type.is_some(), end))
    };
    let (target, typed, after_reference) =
        local_target(start + typed_selection::IDENTITY_REFERENCE)?;
    let (constant_one, asset_at, zero_run, marker, selection_reference) = if typed {
        (
            typed_selection::CONSTANT_ONE,
            typed_selection::LEN,
            typed_chain::ZERO_RUN_14,
            typed_chain::CHAIN_MARKER,
            typed_chain::SELECTION_REFERENCE,
        )
    } else {
        (
            compact_profile_selection::CONSTANT_ONE,
            compact_profile_selection::LEN,
            compact_chain::ZERO_RUN_14,
            compact_chain::CHAIN_MARKER,
            compact_chain::SELECTION_REFERENCE,
        )
    };
    if bytes.get(start + typed_selection::ZERO_RUN_10..start + typed_selection::IDENTITY_REFERENCE)
        != Some(&[0; 10][..])
        || target != u64::from(identity_index)
        || after_reference != start + constant_one
        || View::u32_le_at(bytes, start + constant_one) != Some(1)
    {
        return None;
    }
    let (asset_guid, after_asset) = lp_utf16_bounded(bytes, start + asset_at, 36..=36)?;
    let (owner_text, _) = lp_utf16_bounded(bytes, after_asset, 1..=36)?;
    if !(owner_text.len() <= 20 && owner_text.bytes().all(|byte| byte.is_ascii_digit())
        || crate::bytes::is_guid_hyphenated(&owner_text))
    {
        return None;
    }
    let &chain_at = records.offsets(chain_index).first()?;
    let chain_reference = local_target(chain_at + selection_reference)?;
    if bytes.get(chain_at + zero_run..chain_at + marker) != Some(&[0; 14][..])
        || bytes.get(chain_at + marker..chain_at + selection_reference)
            != Some(&SKETCHLESS_PROFILE_CHAIN_MARKER[..])
        || chain_reference.0 != u64::from(record_index)
        || chain_reference.1 != typed
    {
        return None;
    }
    Some(asset_guid)
}

/// The `feature.extrude-profile-sketch-absent` note for selection record
/// `selection_record` of Extrude scope record `scope_record`.
pub(super) fn sketchless_profile_selection_loss(
    selection_record: u32,
    scope_record: u32,
    asset_guid: &str,
    design_asset_guid: Option<&str>,
) -> LossNote {
    let foreign =
        design_asset_guid.is_some_and(|design_guid| !design_guid.eq_ignore_ascii_case(asset_guid));
    let message = if foreign {
        format!(
            "Profile selection record {selection_record} of Extrude scope record {scope_record} has a chain record that names no Sketch; its asset GUID {asset_guid} is not the Design's asset GUID, and the profile stays native."
        )
    } else {
        format!(
            "Profile selection record {selection_record} of Extrude scope record {scope_record} has a chain record that names no Sketch, and the profile stays native."
        )
    };
    crate::loss::F3dLossCode::FeatureExtrudeProfileSketchAbsent.note(message)
}

/// One `feature.extrude-profile-sketch-absent` note for each Extrude profile
/// member that stays native and whose typed-reference or compact selection
/// has a sketchless chain record.
pub(crate) fn sketchless_profile_selection_losses(
    scan: &ContainerScan,
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    headers: &[DesignRecordHeader],
    features: &[cadmpeg_ir::features::Feature],
) -> Result<Vec<LossNote>, CodecError> {
    use cadmpeg_ir::features::profiles::{PlanarProfileRef, ProfileRef};
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};

    let headers = indexed_operand_headers(headers);
    let native_profiles = features
        .iter()
        .filter(|feature| {
            matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Extrude {
                    profile: ProfileRef::Planar(PlanarProfileRef::Native(_)),
                    ..
                })
            )
        })
        .filter_map(|feature| feature.native_ref.as_deref())
        .collect::<HashSet<_>>();
    let extrude_scopes = scopes
        .iter()
        .filter(|scope| {
            design_feature_family(&scope.kind()) == Some(DesignFeatureFamily::Extrude)
                && native_profiles.contains(scope.id.as_str())
        })
        .filter_map(|scope| Some((native_stream(&scope.id)?, scope.record_index)))
        .collect::<HashSet<_>>();
    let mut records_by_stream = HashMap::new();
    let mut losses = Vec::new();
    for group in groups {
        let Some(stream) = native_stream(&group.id) else {
            continue;
        };
        if group.extrude_role() != Some(DesignExtrudeOperandRole::Profile)
            || !extrude_scopes.contains(&(stream, group.scope_record_index))
        {
            continue;
        }
        let Some(entry) = scan.design_stream_entry_for_scope(ContainerRole::Bulkstream, stream)
        else {
            continue;
        };
        let bytes = scan.entry_bytes(&entry.name)?;
        for member in group.members() {
            let Some(header) = headers.get(&(stream, member.value)) else {
                continue;
            };
            let records = records_by_stream
                .entry(stream)
                .or_insert_with(|| IndexedRecordOffsets::build(bytes));
            let Some(asset_guid) = sketchless_profile_selection_asset(bytes, records, header)
            else {
                continue;
            };
            losses.push(sketchless_profile_selection_loss(
                header.record_index,
                group.scope_record_index,
                &asset_guid,
                scan.design_asset_guid(),
            ));
        }
    }
    Ok(losses)
}
