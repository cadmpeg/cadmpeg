// SPDX-License-Identifier: Apache-2.0
//! Validate Sketch placement and profile links.

use cadmpeg_core::decode::u64_from_index;

use super::{design_stream, Ctx};
use crate::layout::sketch_profile_region_selection_prefix as region_selection;
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::sketch_profile::{
    DesignSketchProfileOperand, DesignSketchProfileRegion, DesignSketchProfileRegionSelection,
};

pub(super) fn placement_link(ctx: &Ctx<'_, '_>, scope: &DesignParameterScope) -> bool {
    let stream = design_stream(&scope.id);
    if scope.sketch_entity().is_none()
        || ctx
            .placements_by_scope
            .contains_key(&(stream, scope.record_index))
    {
        return true;
    }
    let mut placements = ctx
        .native
        .design_placement_constructions
        .iter()
        .filter(|placement| {
            design_stream(&placement.id) == stream
                && scope
                    .reference_members()
                    .values()
                    .any(|index| u64::from(*index) == placement.record_index)
        });
    let Some(placement) = placements.next() else {
        return false;
    };
    placements.next().is_none()
        && u32::try_from(placement.record_index)
            .ok()
            .and_then(|index| ctx.records_by_index.get(&(stream, index)))
            .is_some_and(|header| {
                header.byte_offset == placement.byte_offset
                    && header.class_tag == placement.class_tag
            })
        && ctx.native.design_types.iter().any(|kind| {
            crate::ids::design_segment(kind.id()) == crate::ids::design_segment(&scope.id)
                && kind
                    .type_guid
                    .as_str()
                    .eq_ignore_ascii_case("7B6C3C2D-4096-4E6D-B225-699E8C2D9355")
                && kind.version == placement.type_version
                && kind.module == "Fusion"
                && kind.base_type_guid.value().is_some_and(|base| {
                    base.as_str()
                        .eq_ignore_ascii_case("2CA5A1CD-C99B-4C9B-91AF-57989148E841")
                })
                && kind
                    .entities
                    .values()
                    .any(|index| *index == placement.record_index)
        })
}

pub(super) fn profile_region_link(
    scope: &DesignParameterScope,
    profile: &DesignSketchProfileOperand,
    selection: &DesignSketchProfileRegionSelection,
) -> bool {
    /// The offset after `loops`, which start with their loop count at
    /// `cursor` and each end with an exterior byte.
    fn loops_end(cursor: u64, loops: &[&DesignSketchProfileRegion]) -> Option<u64> {
        let mut cursor = cursor.checked_add(4)?;
        if loops.is_empty() {
            return None;
        }
        for region in loops {
            if region.member_count_offset != cursor || region.members.is_empty() {
                return None;
            }
            cursor = cursor.checked_add(4)?;
            for member in &region.members {
                if member.kind_offset != cursor
                    || member.curve_primary_id_offset != cursor.checked_add(4)?
                    || member.incidence_words_offset != cursor.checked_add(8)?
                {
                    return None;
                }
                cursor = cursor.checked_add(40)?;
            }
            cursor = cursor.checked_add(1)?;
        }
        Some(cursor)
    }

    let Some(expected_region_count_offset) = selection
        .byte_offset
        .checked_add(u64_from_index(region_selection::LOOP_COUNT))
    else {
        return false;
    };
    let selection_index_matches = profile.record_index.checked_add(3)
        == Some(selection.record_index)
        || (profile.record_index.checked_add(2) == Some(selection.record_index)
            && profile.scope_reference_ordinal == 1
            && profile.class_tag.as_str() == "272"
            && profile.paired_class_tag().as_str() == "262"
            && scope.is_legacy_class_330_symmetric_distance());
    if !selection_index_matches
        || selection.byte_offset <= profile.paired_byte_offset()
        || selection.region_count_offset != expected_region_count_offset
        || selection.regions.is_empty()
    {
        return false;
    }
    let walk = || -> Option<u64> {
        let mut cursor = selection.region_count_offset;
        let mut start = 0;
        let mut ordinal = 0;
        let mut atomic_seen = 0;
        while start < selection.regions.len() {
            let own = selection
                .regions
                .iter()
                .skip(start)
                .take_while(|region| region.region_ordinal == ordinal)
                .collect::<Vec<_>>();
            start = start.checked_add(own.len())?;
            cursor = loops_end(cursor, &own)?;
            cursor = cursor.checked_add(4)?;
            for atomic in selection
                .atomic_regions
                .iter()
                .filter(|atomic| atomic.region_ordinal == ordinal)
            {
                if atomic.loop_count_offset != cursor {
                    return None;
                }
                cursor = loops_end(cursor, &atomic.loops.iter().collect::<Vec<_>>())?;
                atomic_seen += 1;
            }
            ordinal = ordinal.checked_add(1)?;
        }
        (atomic_seen == selection.atomic_regions.len()).then_some(cursor)
    };
    walk() == Some(selection.companion_byte_offset)
}

#[cfg(test)]
mod tests;
