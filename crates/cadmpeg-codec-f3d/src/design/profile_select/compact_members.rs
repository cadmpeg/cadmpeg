// SPDX-License-Identifier: Apache-2.0
//! Certify a compact group's initial boundary across sketch subdivisions.

use super::{
    cap_regions, profile_curve_selection_curves, stored_loop_rings, EntitySelectionPathResolution,
    ScopedExtrudeProfileResolution,
};
use crate::records::topology::construction::DesignConstructionOperandGroup;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{PlanarProfileRef, SketchProfileBoundaryUse, SketchProfileRegion};

pub(super) fn resolve(
    group: &DesignConstructionOperandGroup,
    resolution: &EntitySelectionPathResolution<'_>,
    scoped: ScopedExtrudeProfileResolution<'_>,
    states: (i64, i64),
    sweep: cap_regions::compact::Sweep<'_>,
) -> Result<Option<PlanarProfileRef>, CodecError> {
    let Some(stream) = crate::ids::native_stream(&group.id) else {
        return Ok(None);
    };
    let mut selections = resolution.profile_curves.iter().filter(|selection| {
        crate::ids::native_stream(&selection.id) == Some(stream)
            && selection.scope_record_index == group.scope_record_index
            && selection.group_record_index == group.record_index
    });
    let Some(selection) = selections.next() else {
        return Ok(None);
    };
    if selections.next().is_some() || !selection.compact_references() {
        return Ok(None);
    }
    let [stored] = selection.groups.as_slice() else {
        return Ok(None);
    };
    if stored
        .region
        .iter()
        .flat_map(|ring| &ring.entries)
        .any(|entry| !matches!(entry.entry_fields, [0 | 1, 1, 1]))
    {
        return Ok(None);
    }
    let Some((sketch_id, curves)) = profile_curve_selection_curves(group, stream, resolution, true)
    else {
        return Ok(None);
    };
    let Some(sketch) = resolution
        .sketches
        .iter()
        .find(|sketch| sketch.id == sketch_id)
    else {
        return Ok(None);
    };
    let Some(count) = stored
        .region
        .iter()
        .try_fold(0usize, |count, ring| count.checked_add(ring.entries.len()))
    else {
        return Ok(None);
    };
    let Some(curves) = curves.get(..count) else {
        return Ok(None);
    };
    let Some(rings) = stored_loop_rings(&stored.region, curves) else {
        return Ok(None);
    };
    let entities = resolution
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == sketch_id && curves.contains(entity.id()))
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
    if initial.coincident
        || initial.island_absence
        || initial.regions.len() != 1
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
        let candidates: Vec<_> = if ring.outer {
            vec![outer_boundary]
        } else {
            hole_boundaries.iter().collect()
        };
        if candidates
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
    let candidate = match sweep.operation {
        cadmpeg_ir::features::BooleanOp::Cut => {
            cap_regions::compact::removed_candidate(sketch, scoped, states, sweep)?
        }
        _ => cap_regions::compact::added_candidate(sketch, scoped, states, sweep)?,
    };
    let Some(candidate) = candidate else {
        return Ok(None);
    };
    let PlanarProfileRef::SketchRegions { regions, .. } = &candidate else {
        return Ok(None);
    };
    Ok(boundaries_agree(&initial.regions, regions).then_some(candidate))
}

fn boundaries_agree(initial: &[SketchProfileRegion], selected: &[SketchProfileRegion]) -> bool {
    match (outline(initial), outline(selected)) {
        (Some(initial), Some(selected)) => initial == selected,
        _ => false,
    }
}

fn outline(regions: &[SketchProfileRegion]) -> Option<Vec<SketchProfileBoundaryUse>> {
    let mut counts = std::collections::HashMap::<_, [usize; 2]>::new();
    for region in regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            return None;
        };
        for use_ in outer_boundary
            .iter()
            .chain(hole_boundaries.iter().flatten())
        {
            let [start, end] = use_.parameter_range.endpoints();
            if start >= end {
                return None;
            }
            let counts = counts
                .entry((&use_.entity, [start.to_bits(), end.to_bits()]))
                .or_default();
            let count = &mut counts[usize::from(use_.reversed)];
            *count = count.checked_add(1)?;
            if *count > 1 {
                return None;
            }
        }
    }
    let mut uses = Vec::new();
    for ((entity, range), counts) in counts {
        let reversed = match counts {
            [1, 0] => false,
            [0, 1] => true,
            [1, 1] => continue,
            _ => return None,
        };
        uses.push(SketchProfileBoundaryUse {
            entity: entity.clone(),
            parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new(
                range.map(f64::from_bits),
            )
            .ok()?,
            reversed,
        });
    }
    uses.sort_by(|a, b| {
        a.entity
            .as_str()
            .cmp(b.entity.as_str())
            .then(a.reversed.cmp(&b.reversed))
            .then(a.parameter_range.endpoints()[0].total_cmp(&b.parameter_range.endpoints()[0]))
    });
    let mut merged: Vec<SketchProfileBoundaryUse> = Vec::new();
    for use_ in uses {
        if let Some(last) = merged
            .last_mut()
            .filter(|last| last.entity == use_.entity && last.reversed == use_.reversed)
        {
            let [start, end] = last.parameter_range.endpoints();
            let [next, far] = use_.parameter_range.endpoints();
            if next < end {
                return None;
            }
            if next == end {
                last.parameter_range =
                    cadmpeg_ir::geometry::DirectedParameterRange::new([start, far]).ok()?;
                continue;
            }
        }
        merged.push(use_);
    }
    Some(merged)
}

#[cfg(test)]
mod tests;
