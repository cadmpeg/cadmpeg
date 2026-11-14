// SPDX-License-Identifier: Apache-2.0
//! Validate the placement carrier of a bound Sketch scope.

use super::{design_stream, Ctx};
use crate::records::feature::scope::DesignParameterScope;

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

#[cfg(test)]
mod tests;
