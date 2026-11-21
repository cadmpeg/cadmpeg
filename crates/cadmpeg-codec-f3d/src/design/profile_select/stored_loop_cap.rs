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
use cadmpeg_ir::features::{BooleanOp, PlanarProfileRef, SketchProfileRegion};
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
        profile_curve_selection_candidates(group, stream, resolution, compact)
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
    Ok(candidate.and_then(|candidate| certify_boundaries(candidate, curves, scoped)))
}

fn certify_boundaries(
    candidate: PlanarProfileRef,
    curves: &[Vec<SketchEntityId>],
    scoped: ScopedExtrudeProfileResolution<'_>,
) -> Option<(PlanarProfileRef, Vec<F3dLossCode>)> {
    let PlanarProfileRef::SketchRegions { regions, .. } = &candidate else {
        return None;
    };
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
            return None;
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
    Some((candidate, losses))
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
