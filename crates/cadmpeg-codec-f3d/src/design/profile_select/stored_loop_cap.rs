// SPDX-License-Identifier: Apache-2.0
//! Resolve a typed curve-loop selection through its certified Extrude transition.

use super::{
    cap_regions, profile_curve_selection_candidates, EntitySelectionPathResolution,
    ScopedExtrudeProfileResolution,
};
use crate::ids::native_stream;
use crate::loss::F3dLossCode;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::operation_settings::BooleanOp;
use cadmpeg_ir::features::profiles::{PlanarProfileRef, SketchProfileRegion};
use cadmpeg_ir::sketches::{Sketch, SketchEntityId};

pub(super) fn resolve(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    scoped: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: cap_regions::compact::Sweep<'_>,
) -> Result<Option<(PlanarProfileRef, Vec<F3dLossCode>)>, CodecError> {
    let Some(stream) = native_stream(&group.id) else {
        return Ok(None);
    };
    let compact = resolution.profile_curves.iter().any(|selection| {
        native_stream(&selection.id) == Some(stream)
            && selection.scope_record_index == group.scope_record_index
            && selection.group_record_index == group.record_index
            && selection.compact_references()
    });
    let Some((sketch, curves)) =
        profile_curve_selection_candidates(group, stream, resolution, compact, false)
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
) -> Result<Option<(PlanarProfileRef, Vec<F3dLossCode>)>, CodecError> {
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
    let curves = named
        .into_iter()
        .map(|entity| vec![entity])
        .collect::<Vec<_>>();
    let Some((candidate, mut losses)) =
        certified_regions_bounded_by(sketch, &curves, scoped, states, sweep)?
    else {
        return Ok(None);
    };
    let PlanarProfileRef::SketchRegions { regions, .. } = &candidate else {
        return Ok(None);
    };
    if !super::compact_members::boundaries_agree(&initial.regions, regions) {
        return Ok(None);
    }
    losses.push(F3dLossCode::FeatureProfileRegionIdentitiesStale);
    Ok(Some((candidate, losses)))
}

/// The exact reading requires every curve to bound a selected region.
/// Split entries and partial boundary matches require a loss.
pub(super) fn certified_regions_bounded_by(
    sketch: &Sketch,
    curves: &[Vec<SketchEntityId>],
    scoped: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: cap_regions::compact::Sweep<'_>,
) -> Result<Option<(PlanarProfileRef, Vec<F3dLossCode>)>, CodecError> {
    let candidate = match sweep.operation {
        BooleanOp::Cut => cap_regions::compact::removed_candidate(sketch, scoped, states, sweep)?,
        _ => cap_regions::compact::added_candidate(sketch, scoped, states, sweep)?,
    };
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let PlanarProfileRef::SketchRegions { regions, .. } = &candidate else {
        return Ok(None);
    };
    if !cap_regions::overlap::regions_are_disjoint(
        regions,
        scoped.entities,
        scoped.linear_tolerance,
        scoped.ctx,
    )? {
        return Ok(None);
    }
    let mut coincident_boundary = false;
    let mut partial_entry = false;
    for pieces in curves {
        partial_entry |=
            pieces.len() > 1 || pieces.iter().any(|curve| !bounds_region(regions, curve));
        if pieces.iter().any(|curve| bounds_region(regions, curve)) {
            continue;
        }
        let overlaps = pieces.iter().any(|curve| {
            scoped
                .entities
                .iter()
                .find(|entity| entity.id() == curve)
                .is_some_and(|entity| {
                    regions.iter().any(|region| match region {
                        SketchProfileRegion::Trimmed {
                            outer_boundary,
                            hole_boundaries,
                        } => outer_boundary
                            .iter()
                            .chain(hole_boundaries.iter().flatten())
                            .any(|use_| {
                                crate::design::geometry::curve_overlaps_boundary_use(
                                    entity,
                                    use_,
                                    scoped.entities,
                                    scoped.linear_tolerance,
                                )
                            }),
                        SketchProfileRegion::Loops { .. } => false,
                    })
                })
        });
        if !overlaps {
            return Ok(None);
        }
        coincident_boundary = true;
    }
    let mut losses = Vec::new();
    if partial_entry {
        losses.push(F3dLossCode::FeatureProfilePartialEntryAssumed);
    }
    if coincident_boundary {
        losses.push(F3dLossCode::FeatureProfileCoincidentBoundaryAssumed);
    }
    Ok(Some((candidate, losses)))
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
