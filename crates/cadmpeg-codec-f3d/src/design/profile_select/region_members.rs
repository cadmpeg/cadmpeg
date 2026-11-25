// SPDX-License-Identifier: Apache-2.0
//! Resolve counted partial-curve member rings to atomic regions.

use super::{sketch_profile_member_entity, ScopedExtrudeProfileResolution};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::profiles::PlanarProfileRef;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchEntityUse, SketchGeometryDefinition};

pub(super) fn resolve(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &Sketch,
    curves: &[SketchCurveIdentity],
    entities: &[SketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    allow_assumptions: bool,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    resolve_with_bridge(
        stream,
        profile,
        sketch,
        curves,
        entities,
        resolution,
        (allow_assumptions, None),
    )
}

fn resolve_with_bridge(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &Sketch,
    curves: &[SketchCurveIdentity],
    entities: &[SketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
    policy: (bool, Option<SketchEntityUse>),
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let (allow_assumptions, bridge) = policy;
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
            members.push((
                SketchEntityUse {
                    entity: entity.clone(),
                    reversed: member.incidence_flag,
                },
                member.incidence_values.map(u32::from),
            ));
        }
        rings.push(members);
    }
    if let Some(bridge) = bridge {
        let [ring] = rings.as_mut_slice() else {
            return Ok(None);
        };
        ring.push((bridge, [1, 1]));
    }
    let Some(selected) = crate::design::geometry::arrangement_regions_matching_counted_rings(
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
        if selected.joined_rings != 0 {
            resolution.profile_work.note_piece_partition_assumed();
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
    regions: &[cadmpeg_ir::features::profiles::SketchProfileRegion],
    assignments: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for (target, region) in regions.iter().enumerate() {
        let cadmpeg_ir::features::profiles::SketchProfileRegion::Trimmed {
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

/// Close a counted member ring that lists every curve of a region except one
/// arc, by adding that arc to the ring.
///
/// The arc is added only when the listed members close no region at all: a
/// ring that closes a region once assumptions are allowed was refused by an
/// assumption gate, not by a missing curve. The arc is the one unlisted
/// non-construction curve whose two ends meet two different listed endpoints;
/// a second such arc, or a line that fits, leaves the selection native. A
/// unique line is not added: a stored selection omits a line as readily as it
/// omits a curve of an unselected neighbour region, so the line does not
/// identify the missing curve the way an arc closing a corner does. Curves
/// that a counted ring never names (tail marker `1`) are not candidates.
pub(super) fn resolve_arc_bridges(
    stream: &str,
    profile: &DesignSketchProfileOperand,
    sketch: &Sketch,
    curves: &[SketchCurveIdentity],
    entities: &[SketchEntity],
    resolution: ScopedExtrudeProfileResolution<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(selection) = profile.planar_region_selection() else {
        return Ok(None);
    };
    let Some(regions) = selection.resolution_regions() else {
        return Ok(None);
    };
    let Ok(owner) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let flags = resolution.profile_work.assumed_flags();
    let inferred = resolution.profile_work.inferred_counts();
    let closes_without_bridge = resolve_with_bridge(
        stream,
        profile,
        sketch,
        curves,
        entities,
        resolution,
        (true, None),
    )?
    .is_some();
    resolution.profile_work.set_assumed_flags(flags);
    resolution.profile_work.set_inferred_counts(inferred);
    if closes_without_bridge {
        return Ok(None);
    }
    let mut excluded = unnamed_curves(curves, sketch, entities);
    let named = regions
        .iter()
        .flat_map(|region| &region.members)
        .filter_map(|member| {
            sketch_profile_member_entity(stream, owner, member, sketch, curves, entities).cloned()
        })
        .collect::<std::collections::HashSet<_>>();
    excluded.extend(named.iter().cloned());
    let mut endpoints = Vec::new();
    for entity in entities.iter().filter(|entity| named.contains(entity.id())) {
        if let Some(ends) =
            crate::design::geometry::sketch_entity_endpoints(entity, resolution.ctx)?
        {
            endpoints.extend(ends);
        }
    }
    let meets = |end: &Point2| {
        endpoints
            .iter()
            .any(|point| (end.u - point.u).hypot(end.v - point.v) <= resolution.linear_tolerance)
    };
    let mut candidates = Vec::new();
    for entity in entities.iter().filter(|entity| {
        entity.sketch == sketch.id && !entity.construction && !excluded.contains(entity.id())
    }) {
        let is_arc = match entity.geometry.definition() {
            SketchGeometryDefinition::Arc { .. } => true,
            SketchGeometryDefinition::Line { .. } => false,
            _ => continue,
        };
        let Some([start, end]) =
            crate::design::geometry::sketch_entity_endpoints(entity, resolution.ctx)?
        else {
            continue;
        };
        let apart = (start.u - end.u).hypot(start.v - end.v) > resolution.linear_tolerance;
        if apart && meets(&start) && meets(&end) {
            candidates.push((entity.id().clone(), is_arc));
        }
    }
    let [(bridge, true)] = candidates.as_slice() else {
        return Ok(None);
    };
    for reversed in [false, true] {
        let selected = resolve_with_bridge(
            stream,
            profile,
            sketch,
            curves,
            entities,
            resolution,
            (
                true,
                Some(SketchEntityUse {
                    entity: bridge.clone(),
                    reversed,
                }),
            ),
        )?;
        if selected.is_some() {
            return Ok(selected);
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests;
