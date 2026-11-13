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
    let Some(selection) = &profile.region_selection else {
        return Ok(None);
    };
    let Ok(owner) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let mut rings = Vec::new();
    for region in &selection.regions {
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
        resolution.linear_tolerance,
        resolution.profile_work,
        resolution.ctx,
    )?
    else {
        return Ok(None);
    };
    if !allow_assumptions && (selected.coincident || selected.island_absence) {
        return Ok(None);
    }
    let profile = PlanarProfileRef::sketch_regions(sketch.id.clone(), selected.regions).ok();
    if profile.is_some() {
        if selected.coincident {
            resolution.profile_work.note_coincident_direction_assumed();
        }
        if selected.island_absence {
            resolution.profile_work.note_island_absence_assumed();
        }
    }
    Ok(profile)
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
    let Some(selection) = &profile.region_selection else {
        return Ok(None);
    };
    let Ok(owner) = u32::try_from(profile.entity_id.suffix()) else {
        return Ok(None);
    };
    let mut stale = false;
    let mut owned = Vec::new();
    for region in &selection.regions {
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
