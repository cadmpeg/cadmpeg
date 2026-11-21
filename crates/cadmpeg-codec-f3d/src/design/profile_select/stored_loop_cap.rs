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
    let candidate = certified_regions_bounded_by(source, &curves, scoped, states, sweep)?;
    if candidate.is_some() {
        return Ok(candidate);
    }
    let mut selections = resolution.profile_curves.iter().filter(|selection| {
        native_stream(&selection.id) == Some(stream)
            && selection.scope_record_index == group.scope_record_index
            && selection.group_record_index == group.record_index
    });
    let Some(selection) = selections.next() else {
        return Ok(None);
    };
    if selections.next().is_some() {
        return Ok(None);
    }
    let [stored] = selection.groups.as_slice() else {
        return Ok(None);
    };
    let [ring] = stored.region.as_slice() else {
        return Ok(None);
    };
    if !ring.outer
        || !stored.further.is_empty()
        || ring.entries.iter().any(|entry| {
            let [flag, ordinal, count] = entry.entry_fields;
            flag > 1 || ordinal == 0 || ordinal > count
        })
    {
        return Ok(None);
    }
    cap_regions::joined::candidate(source, scoped, states, sweep, &curves)
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
