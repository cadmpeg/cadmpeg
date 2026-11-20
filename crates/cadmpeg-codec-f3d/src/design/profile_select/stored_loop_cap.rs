// SPDX-License-Identifier: Apache-2.0
//! Resolve a typed curve-loop selection through its certified Extrude transition.

use super::{
    cap_regions, profile_curve_selection_curves, EntitySelectionPathResolution,
    ScopedExtrudeProfileResolution,
};
use crate::ids::native_stream;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{BooleanOp, PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId};

/// The regions that the certified Extrude transition from `states.1` to
/// `states.0` selects for a typed curve-loop selection, when each curve that
/// the stored loops name bounds a selected region.
///
/// A Cut takes the atomic regions it removed from the planes it changed; a
/// Join or `NewBody` takes the atomic regions its far end cap covers. Both
/// readings pass the exact transition certificate. `None` when a stored curve
/// has no unique sketch curve, the transition certifies no region set, or a
/// stored curve bounds no selected region.
pub(super) fn resolve(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    scoped: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: cap_regions::compact::Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(stream) = native_stream(&group.id) else {
        return Ok(None);
    };
    let compact = resolution.profile_curves.iter().any(|selection| {
        native_stream(&selection.id) == Some(stream)
            && selection.scope_record_index == group.scope_record_index
            && selection.group_record_index == group.record_index
            && selection.compact_references()
    });
    let Some((sketch, curves)) = profile_curve_selection_curves(group, stream, resolution, compact)
    else {
        return Ok(None);
    };
    let Some(source) = resolution
        .sketches
        .iter()
        .find(|source| source.id == sketch)
    else {
        return Ok(None);
    };
    certified_regions_bounded_by(source, &curves, scoped, states, sweep)
}

pub(super) fn resolve_stale(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    scoped: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: cap_regions::compact::Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(stream) = native_stream(&group.id) else {
        return Ok(None);
    };
    let [member] = group.members() else {
        return Ok(None);
    };
    let mut selections = resolution.profile_curves.iter().filter(|selection| {
        native_stream(&selection.id) == Some(stream)
            && selection.scope_record_index == group.scope_record_index
            && selection.group_record_index == group.record_index
    });
    let Some(selection) = selections.next() else {
        return Ok(None);
    };
    if selections.next().is_some()
        || selection.record_index != member.value
        || selection.compact_references()
    {
        return Ok(None);
    }
    let [stored] = selection.groups.as_slice() else {
        return Ok(None);
    };
    if !stored.further.is_empty() {
        return Ok(None);
    }
    let Some((sketch, entries)) =
        super::profile_curve_selection_candidates(group, stream, resolution, false, true)
    else {
        return Ok(None);
    };
    if !entries.iter().any(Vec::is_empty) || entries.iter().any(|ids| ids.len() > 1) {
        return Ok(None);
    }
    let Some(sketch) = resolution
        .sketches
        .iter()
        .find(|source| source.id == sketch)
    else {
        return Ok(None);
    };
    let mut rings = Vec::new();
    let mut entries = entries.into_iter();
    let mut named = Vec::new();
    for ring in &stored.region {
        let mut members = Vec::new();
        for entry in &ring.entries {
            let [direction @ (0 | 1), _, _] = entry.entry_fields else {
                return Ok(None);
            };
            let Some(ids) = entries.next() else {
                return Ok(None);
            };
            if let Some(entity) = ids.into_iter().next() {
                if named.contains(&entity) {
                    return Ok(None);
                }
                named.push(entity.clone());
                members.push(cadmpeg_ir::sketches::SketchEntityUse {
                    entity,
                    reversed: direction == 1,
                });
            }
        }
        if members.is_empty() {
            return Ok(None);
        }
        rings.push(members);
    }
    if entries.next().is_some() {
        return Ok(None);
    }
    let entities = resolution
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id && named.contains(entity.id()))
        .cloned()
        .collect::<Vec<_>>();
    let work = crate::design::geometry::ProfileGeometryWork::new();
    let Some(initial) = crate::design::geometry::arrangement_regions_matching_member_rings(
        sketch,
        &entities,
        &rings,
        &std::collections::HashSet::new(),
        scoped.linear_tolerance,
        &work,
        scoped.ctx,
    )?
    else {
        return Ok(None);
    };
    if initial.regions.len() != 1
        || !initial.identity_unique
        || initial.coincident
        || initial.island_absence
        || work.assumed_flags().into_iter().any(|assumed| assumed)
    {
        return Ok(None);
    }
    let SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    } = &initial.regions[0]
    else {
        return Ok(None);
    };
    for (ring, members) in stored.region.iter().zip(&rings) {
        let boundaries = if ring.outer {
            std::slice::from_ref(outer_boundary)
        } else {
            hole_boundaries.as_slice()
        };
        if boundaries
            .iter()
            .filter(|boundary| {
                boundary.len() == members.len()
                    && members.iter().all(|member| {
                        boundary
                            .iter()
                            .filter(|use_| use_.entity == member.entity)
                            .count()
                            == 1
                    })
            })
            .count()
            != 1
        {
            return Ok(None);
        }
    }
    let Some(candidate) = certified_regions_bounded_by(sketch, &named, scoped, states, sweep)?
    else {
        return Ok(None);
    };
    let PlanarProfileRef::SketchRegions { regions, .. } = &candidate else {
        return Ok(None);
    };
    Ok(super::compact_members::boundaries_agree(&initial.regions, regions).then_some(candidate))
}

/// The region set the transition certifies in `sketch`, when each of
/// `curves` bounds one of its regions.
pub(super) fn certified_regions_bounded_by(
    sketch: &Sketch,
    curves: &[SketchEntityId],
    scoped: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: cap_regions::compact::Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let candidate = match sweep.operation {
        BooleanOp::Cut => cap_regions::compact::removed_candidate(sketch, scoped, states, sweep)?,
        _ => cap_regions::compact::added_candidate(sketch, scoped, states, sweep)?,
    };
    Ok(candidate.filter(|candidate| {
        matches!(candidate, PlanarProfileRef::SketchRegions { regions, .. }
            if curves.iter().all(|curve| bounds_region(regions, curve)))
    }))
}

/// Whether `curve` has a use in the exterior or a hole ring of a region.
fn bounds_region(regions: &[SketchProfileRegion], curve: &SketchEntityId) -> bool {
    regions.iter().any(|region| match region {
        SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } => outer_boundary
            .iter()
            .chain(hole_boundaries.iter().flat_map(|ring| ring.iter()))
            .any(|use_| use_.entity == *curve),
        SketchProfileRegion::Loops { .. } => false,
    })
}

#[cfg(test)]
mod tests;
