// SPDX-License-Identifier: Apache-2.0
//! Resolve counted partial-curve member rings to atomic regions.

use super::{sketch_profile_member_entity, ScopedExtrudeProfileResolution};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::PlanarProfileRef;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityUse};

pub(super) fn resolve(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &Sketch,
    curves: &[SketchCurveIdentity],
    entities: &[SketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    allow_assumptions: bool,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(selection) = profile.planar_region_selection() else {
        return Ok(None);
    };
    let Ok(owner) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let Some(regions) = selection.resolution_regions() else {
        return Ok(None);
    };
    let mut rings = Vec::new();
    for region in regions {
        let mut members = Vec::new();
        for member in &region.members {
            let Some(entity) =
                sketch_profile_member_entity(stream, owner, member, sketch, curves, entities)
            else {
                return Ok(None);
            };
            members.push(SketchEntityUse {
                entity: entity.clone(),
                reversed: member.incidence_flag,
            });
        }
        rings.push(members);
    }
    let Some(selected) = crate::design::geometry::arrangement_regions_matching_member_rings(
        sketch,
        entities,
        &rings,
        &unnamed_curves(curves, sketch, entities),
        resolution.linear_tolerance,
        resolution.profile_work,
        resolution.ctx,
    )?
    else {
        return Ok(None);
    };
    let crossing_runs_assumed = selected.crossing_runs_assumed();
    if !allow_assumptions
        && ((selected.coincident && !selected.identity_unique)
            || selected.island_absence
            || crossing_runs_assumed)
    {
        return Ok(None);
    }
    let profile = PlanarProfileRef::sketch_regions(sketch.id.clone(), selected.regions).ok();
    if profile.is_some() {
        if crossing_runs_assumed {
            resolution.profile_work.note_crossing_runs_assumed();
        }
        if selected.coincident && (allow_assumptions || !selected.identity_unique) {
            resolution.profile_work.note_coincident_direction_assumed();
        }
        if selected.island_absence {
            resolution.profile_work.note_island_absence_assumed();
        }
    }
    Ok(profile)
}

/// The entities of `sketch` whose curve records carry tail marker `1`. A
/// counted region member list never names them.
fn unnamed_curves(
    curves: &[SketchCurveIdentity],
    sketch: &Sketch,
    entities: &[SketchEntity],
) -> std::collections::HashSet<cadmpeg_ir::sketches::SketchEntityId> {
    let marked = curves
        .iter()
        .filter(|curve| curve.tail_marker == Some(1))
        .map(|curve| curve.id.as_str())
        .collect::<std::collections::HashSet<_>>();
    entities
        .iter()
        .filter(|entity| entity.sketch == sketch.id)
        .filter(|entity| {
            entity
                .native_ref
                .as_deref()
                .is_some_and(|native| marked.contains(native))
        })
        .map(|entity| entity.id().clone())
        .collect()
}

/// The regions that the certified Extrude transition selects, when every
/// curve that the counted member rings name bounds one of them.
///
/// A ring names the curves of one face of the arrangement that omits the
/// curves with tail marker `1`; a face that such a curve splits is several
/// atomic regions, and no atomic region has the ring's curves. The transition
/// certificate (the far cap of a Join or `NewBody`, the removed planes of a
/// Cut) fixes the atomic regions that the Extrude takes; each named curve
/// has to bound one of them. A selection that names a curve identity the
/// Sketch no longer holds is left to the stale-identity rule.
pub(super) fn resolve_certified(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &Sketch,
    curves: &[SketchCurveIdentity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: super::cap_regions::compact::Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(selection) = &profile.region_selection else {
        return Ok(None);
    };
    let Ok(owner) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let Some(regions) = selection.resolution_regions() else {
        return Ok(None);
    };
    let mut named = Vec::new();
    for region in regions {
        for member in &region.members {
            let has_identity = curves.iter().any(|curve| {
                crate::ids::native_stream(&curve.id) == Some(stream)
                    && curve.owner_reference == Some(owner)
                    && curve.primary_id.get() == u64::from(member.curve_primary_id.get())
            });
            if !has_identity {
                return Ok(None);
            }
            let Some(entity) = sketch_profile_member_entity(
                stream,
                owner,
                member,
                sketch,
                curves,
                resolution.entities,
            ) else {
                return Ok(None);
            };
            named.push(vec![entity.clone()]);
        }
    }
    if named.is_empty() {
        return Ok(None);
    }
    Ok(super::stored_loop_cap::certified_regions_bounded_by(
        sketch, &named, resolution, states, sweep,
    )?
    .filter(|(_, losses)| losses.is_empty())
    .map(|(profile, _)| profile))
}

pub(super) fn resolve_stale(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &Sketch,
    curves: &[SketchCurveIdentity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: super::cap_regions::compact::Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(selection) = profile.planar_region_selection() else {
        return Ok(None);
    };
    let Ok(owner) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let mut stale = false;
    let Some(regions) = selection.resolution_regions() else {
        return Ok(None);
    };
    let mut owned = Vec::new();
    for region in regions {
        let mut members = Vec::new();
        for member in &region.members {
            let has_identity = curves.iter().any(|curve| {
                crate::ids::native_stream(&curve.id) == Some(stream)
                    && curve.owner_reference == Some(owner)
                    && curve.primary_id.get() == u64::from(member.curve_primary_id.get())
            });
            if has_identity {
                let Some(entity) = sketch_profile_member_entity(
                    stream,
                    owner,
                    member,
                    sketch,
                    curves,
                    resolution.entities,
                ) else {
                    return Ok(None);
                };
                members.push(entity.clone());
            } else {
                stale = true;
            }
        }
        owned.push(members);
    }
    if !stale {
        return Ok(None);
    }
    let Some(candidate) =
        super::cap_regions::compact::removed_candidate(sketch, resolution, states, sweep)?
    else {
        return Ok(None);
    };
    let PlanarProfileRef::SketchRegions { regions, .. } = &candidate else {
        return Ok(None);
    };
    if regions.len() != owned.len() {
        return Ok(None);
    }
    let mut assignments = vec![None; owned.len()];
    for index in 0..owned.len() {
        let mut visited = vec![false; owned.len()];
        if !assign_region(index, &owned, regions, &mut assignments, &mut visited) {
            return Ok(None);
        }
    }
    Ok(Some(candidate))
}

fn assign_region(
    index: usize,
    owned: &[Vec<cadmpeg_ir::sketches::SketchEntityId>],
    regions: &[cadmpeg_ir::features::SketchProfileRegion],
    assignments: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for (target, region) in regions.iter().enumerate() {
        let cadmpeg_ir::features::SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            continue;
        };
        if visited[target]
            || !owned[index].iter().all(|entity| {
                outer_boundary
                    .iter()
                    .chain(hole_boundaries.iter().flatten())
                    .any(|use_| &use_.entity == entity)
            })
        {
            continue;
        }
        visited[target] = true;
        if assignments[target]
            .is_none_or(|previous| assign_region(previous, owned, regions, assignments, visited))
        {
            assignments[target] = Some(index);
            return true;
        }
    }
    false
}
