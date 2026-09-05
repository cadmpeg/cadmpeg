// SPDX-License-Identifier: Apache-2.0
//! Compact hole and circular-sweep geometry.

use std::collections::BTreeSet;

use cadmpeg_ir::features::{
    BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition as IrFeatureDefinition, Length,
    LinearTermination, ProfileRef,
};
use cadmpeg_ir::geometry::SurfaceGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::container::ContainerScan;

use super::super::sketch::normalized;
use super::super::sweep::{feature_outline_plane, feature_outline_planes, FeatureOutlinePlane};
use super::placement::{
    cap_square_center_radius, cylinder_from_single_cap_outline, hole_cylinder_from_cap_outlines,
    hole_placement, plane_envelope_corners, ExtrusionSpan, PartialCapOutline, SimpleHoleGeometry,
};

const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_CENTER_AGREEMENT: f64 = 1.0e-9;
const EPS_OFFSET_NONZERO: f64 = 1.0e-12;
const EPS_EXTENT_AGREEMENT: f64 = 1.0e-9;

pub fn simple_hole_geometry(scan: &ContainerScan, feature_id: u32) -> Option<SimpleHoleGeometry> {
    let cap_rows = feature_outline_planes(scan, feature_id)?
        .into_iter()
        .map(|(id, origin, normal)| {
            let envelopes = scan
                .planes
                .envelopes
                .iter()
                .filter(|envelope| envelope.surface_id == id)
                .collect::<Vec<_>>();
            let [envelope] = envelopes.as_slice() else {
                return None;
            };
            Some((
                id,
                origin,
                normal,
                plane_envelope_corners(&envelope.envelope)?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = cap_rows.as_slice() else {
        return None;
    };
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id && !table.surface_ids().is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let entry_ids = table.entry_ids();
    let [entry_plane, termination_plane, first_cylinder, second_cylinder] = entry_ids.as_slice()
    else {
        return None;
    };
    if *entry_plane != first.0 || *termination_plane != second.0 {
        return None;
    }
    let cylinder_ids = [*first_cylinder, *second_cylinder];
    if cylinder_ids.iter().any(|id| {
        !crate::surface::unique_surface_row(&scan.surfaces.rows, *id).is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    }) {
        return None;
    }
    let (_, direction, extent) =
        hole_placement([*first, *second].map(|(id, origin, normal, _)| (id, origin, normal)))?;
    Some(SimpleHoleGeometry {
        entry_surface_id: Some(*entry_plane),
        cylinder_ids: cylinder_ids.to_vec(),
        direction,
        extent,
        geometry: hole_cylinder_from_cap_outlines([*first, *second])?,
    })
}

fn has_exact_materialized_surface_roster(
    table: &crate::feature::FeatureEntityTable,
    expected_ids: impl IntoIterator<Item = u32>,
) -> bool {
    let expected_ids = expected_ids.into_iter().collect::<Vec<_>>();
    let expected_set = expected_ids.iter().copied().collect::<BTreeSet<_>>();
    expected_ids.len() == expected_set.len()
        && table.surface_ids().len() == expected_set.len()
        && table.surface_ids().iter().copied().collect::<BTreeSet<_>>() == expected_set
}

pub fn compact_simple_hole_cylinder_id(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<u32> {
    let candidates = tables
        .iter()
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 29)
        .filter_map(|table| {
            let entry_ids = table
                .entries
                .iter()
                .map(|entry| entry.entity_id)
                .collect::<Vec<_>>();
            (table.entry_ids() == entry_ids).then_some(())?;

            let topology_candidates = table
                .entries
                .windows(2)
                .enumerate()
                .filter_map(|(index, pair)| {
                    let [class_204, class_203] = pair else {
                        unreachable!("two-entry window")
                    };
                    (class_204.class_id == 204
                        && class_203.class_id == 203
                        && class_204.source_entity_id().is_none()
                        && class_203.source_entity_id().is_none())
                    .then_some(())?;
                    let planes = pair
                        .iter()
                        .filter(|candidate| {
                            table.surface_ids().contains(&candidate.entity_id)
                                && rows
                                    .iter()
                                    .filter(|row| row.id == candidate.entity_id)
                                    .count()
                                    == 1
                                && rows.iter().any(|row| {
                                    row.id == candidate.entity_id
                                        && row.feature_id == feature_id
                                        && row.kind == crate::surface::SurfaceKind::Plane
                                })
                        })
                        .collect::<Vec<_>>();
                    let plane = match planes.as_slice() {
                        [] if table.entries.len() == 4 => None,
                        [plane] => Some(plane.entity_id),
                        _ => return None,
                    };
                    pair.iter()
                        .filter(|candidate| Some(candidate.entity_id) != plane)
                        .all(|candidate| {
                            !table.surface_ids().contains(&candidate.entity_id)
                                && !rows.iter().any(|row| row.id == candidate.entity_id)
                        })
                        .then_some((index, plane))
                })
                .collect::<Vec<_>>();
            let [(topology_index, plane)] = topology_candidates.as_slice() else {
                return None;
            };
            let bottoms = table
                .entries
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate.class_id == 200
                        && candidate.source_entity_id() == Some(0)
                        && !table.surface_ids().contains(&candidate.entity_id)
                        && !rows.iter().any(|row| row.id == candidate.entity_id)
                })
                .collect::<Vec<_>>();
            let [(bottom_index, _)] = bottoms.as_slice() else {
                return None;
            };
            let sides = table
                .entries
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate.class_id == 200
                        && candidate.source_entity_id().is_none()
                        && table.surface_ids().contains(&candidate.entity_id)
                        && rows
                            .iter()
                            .filter(|row| row.id == candidate.entity_id)
                            .count()
                            == 1
                        && rows.iter().any(|row| {
                            row.id == candidate.entity_id
                                && row.feature_id == feature_id
                                && row.kind == crate::surface::SurfaceKind::Cylinder
                        })
                })
                .collect::<Vec<_>>();
            let [(side_index, side)] = sides.as_slice() else {
                return None;
            };
            let mut expected_materialized = BTreeSet::from([side.entity_id]);
            expected_materialized.extend(*plane);
            (has_exact_materialized_surface_roster(table, expected_materialized.iter().copied())
                && topology_index < bottom_index
                && bottom_index < side_index)
                .then_some(side.entity_id)
        })
        .collect::<Vec<_>>();
    let [cylinder_id] = candidates.as_slice() else {
        return None;
    };
    Some(*cylinder_id)
}

pub fn compact_simple_hole_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<SimpleHoleGeometry> {
    let cylinder_id = compact_simple_hole_cylinder_id(
        feature_id,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    )?;
    let frame = crate::surface::unique_surface_parameter(&scan.surfaces.parameters, cylinder_id)?
        .positional_cylinder_frame?;
    let length = frame.length?;
    Some(SimpleHoleGeometry {
        entry_surface_id: None,
        cylinder_ids: vec![cylinder_id],
        direction: frame.axis,
        extent: LinearTermination::Blind {
            length: Length(length),
        },
        geometry: SurfaceGeometry::Cylinder {
            origin: Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]),
            axis: Vector3::new(frame.axis[0], frame.axis[1], frame.axis[2]),
            ref_direction: Vector3::new(
                frame.ref_direction[0],
                frame.ref_direction[1],
                frame.ref_direction[2],
            ),
            radius: frame.radius,
        },
    })
}

pub fn circular_sweep_cylinder_from_cap_outlines(
    caps: [PartialCapOutline; 2],
) -> Option<SurfaceGeometry> {
    let (_, axis, _) = hole_placement(caps.map(|(id, origin, normal, _)| (id, origin, normal)))?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - EPS_AXIS_ALIGNMENT
            && (0..3).all(|other| other == *index || axis[other].abs() < EPS_AXIS_ALIGNMENT)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let circles = caps
        .iter()
        .filter_map(|(_, _, _, corners)| cap_square_center_radius((*corners)?, axis_index))
        .collect::<Vec<_>>();
    let (center, radius) = circles.first().copied()?;
    let scale = center
        .iter()
        .chain(std::iter::once(&radius))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if circles.iter().skip(1).any(|(other_center, other_radius)| {
        radial.iter().any(|index| {
            (center[*index] - other_center[*index]).abs() > EPS_CENTER_AGREEMENT * scale
        }) || (radius - other_radius).abs() > EPS_CENTER_AGREEMENT * scale
    }) {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub struct CircularSweepGeometry {
    pub cylinder_ids: Vec<u32>,
    pub section_definition_id: Option<u32>,
    pub direction: [f64; 3],
    pub extent: ExtrudeExtent,
    pub geometry: SurfaceGeometry,
}

pub fn single_cap_circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id && !table.surface_ids().is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [first_cap, second_cap, profile_id, cylinder_id] = table.entries.as_slice() else {
        return None;
    };
    let (rowless_cap, cap_id) = match (
        table.surface_ids().contains(&first_cap.entity_id),
        table.surface_ids().contains(&second_cap.entity_id),
    ) {
        (true, false) => (second_cap, first_cap),
        (false, true) => (first_cap, second_cap),
        _ => return None,
    };
    if [
        first_cap.class_id,
        second_cap.class_id,
        profile_id.class_id,
        cylinder_id.class_id,
    ] != [204, 203, 200, 200]
        || profile_id.source_entity_id().is_none()
        || cylinder_id.source_entity_id().is_some()
        || !has_exact_materialized_surface_roster(table, [cap_id.entity_id, cylinder_id.entity_id])
        || !table
            .non_surface_entity_ids()
            .contains(&rowless_cap.entity_id)
        || !table
            .non_surface_entity_ids()
            .contains(&profile_id.entity_id)
    {
        return None;
    }
    crate::surface::unique_surface_row(&scan.surfaces.rows, cap_id.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .then_some(())?;
    crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_id.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
        .then_some(())?;
    let plane = feature_outline_plane(scan, feature_id, cap_id.entity_id)?;
    let envelopes = scan
        .planes
        .envelopes
        .iter()
        .filter(|envelope| envelope.surface_id == cap_id.entity_id)
        .collect::<Vec<_>>();
    let [envelope] = envelopes.as_slice() else {
        return None;
    };
    let cap = (
        plane.0,
        plane.1,
        plane.2,
        plane_envelope_corners(&envelope.envelope),
    );
    let transforms = scan
        .features
        .section_transforms
        .iter()
        .filter(|transform| transform.feature_id == Some(feature_id))
        .collect::<Vec<_>>();
    let [transform] = transforms.as_slice() else {
        return None;
    };
    let (extent, direction) =
        extrusion_extent_and_direction(transform.origin, transform.normal, [(plane.1, plane.2)])?;
    Some(CircularSweepGeometry {
        cylinder_ids: vec![cylinder_id.entity_id],
        section_definition_id: Some(transform.definition_id),
        direction,
        extent,
        geometry: cylinder_from_single_cap_outline(cap)?,
    })
}

pub fn circular_sweep_feature_definition(
    profile: ProfileRef,
    sweep: &CircularSweepGeometry,
    op: BooleanOp,
    solid: Option<bool>,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Extrude {
        profile,
        direction: cadmpeg_ir::features::ExtrudeDirection::Explicit {
            vector: Vector3::new(sweep.direction[0], sweep.direction[1], sweep.direction[2]),
            source: None,
        },
        start: cadmpeg_ir::features::ExtrudeStart::default(),
        extent: sweep.extent.clone(),
        op,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }
}

pub fn circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    two_cap_circular_sweep_geometry(scan, feature_id)
        .or_else(|| single_cap_circular_sweep_geometry(scan, feature_id))
}

pub fn two_cap_circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == feature_id && !table.surface_ids().is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [first_plane_entry, second_plane_entry, profile_entry, cylinder_entry] =
        table.entries.as_slice()
    else {
        return None;
    };
    if table.entry_ids()
        != [
            first_plane_entry.entity_id,
            second_plane_entry.entity_id,
            profile_entry.entity_id,
            cylinder_entry.entity_id,
        ]
        || [
            first_plane_entry.class_id,
            second_plane_entry.class_id,
            profile_entry.class_id,
            cylinder_entry.class_id,
        ] != [204, 203, 200, 200]
        || first_plane_entry.source_entity_id().is_some()
        || second_plane_entry.source_entity_id().is_some()
        || profile_entry.source_entity_id().is_none()
        || cylinder_entry.source_entity_id().is_some()
        || !has_exact_materialized_surface_roster(
            table,
            [
                first_plane_entry.entity_id,
                second_plane_entry.entity_id,
                cylinder_entry.entity_id,
            ],
        )
        || table.surface_ids().contains(&profile_entry.entity_id)
        || !table
            .non_surface_entity_ids()
            .contains(&profile_entry.entity_id)
    {
        return None;
    }
    let first = feature_outline_plane(scan, feature_id, first_plane_entry.entity_id)?;
    let second = feature_outline_plane(scan, feature_id, second_plane_entry.entity_id)?;
    let cap = |plane: FeatureOutlinePlane| {
        let envelopes = scan
            .planes
            .envelopes
            .iter()
            .filter(|envelope| envelope.surface_id == plane.0)
            .collect::<Vec<_>>();
        let corners = match envelopes.as_slice() {
            [envelope] => plane_envelope_corners(&envelope.envelope),
            _ => None,
        };
        (plane.0, plane.1, plane.2, corners)
    };
    if !crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_entry.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    {
        return None;
    }
    let (_, direction, termination) = hole_placement([first, second])?;
    Some(CircularSweepGeometry {
        cylinder_ids: vec![cylinder_entry.entity_id],
        section_definition_id: None,
        direction,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination,
                draft: None,
            },
        },
        geometry: circular_sweep_cylinder_from_cap_outlines([cap(first), cap(second)])?,
    })
}

pub fn extrusion_span(
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<ExtrusionSpan> {
    let direction_length = direction
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if direction_length <= f64::EPSILON {
        return None;
    }
    let direction = direction.map(|value| value / direction_length);
    let mut offsets = Vec::<f64>::new();
    for (origin, normal) in planes {
        let normal_length = normal.iter().map(|value| value * value).sum::<f64>().sqrt();
        if normal_length <= f64::EPSILON {
            continue;
        }
        let parallel = normal
            .iter()
            .zip(direction)
            .map(|(left, right)| left * right)
            .sum::<f64>()
            .abs();
        if (parallel / normal_length - 1.0).abs() > EPS_AXIS_ALIGNMENT {
            continue;
        }
        let offset = origin
            .iter()
            .zip(profile_origin)
            .zip(direction)
            .map(|((coordinate, base), axis)| (coordinate - base) * axis)
            .sum::<f64>();
        if offset.abs() <= EPS_OFFSET_NONZERO {
            continue;
        }
        let scale = offset.abs().max(1.0);
        if !offsets
            .iter()
            .any(|known| (known - offset).abs() <= EPS_EXTENT_AGREEMENT * scale)
        {
            offsets.push(offset);
        }
    }
    let lower = offsets
        .iter()
        .copied()
        .filter(|offset| *offset < 0.0)
        .min_by(f64::total_cmp);
    let upper = offsets
        .iter()
        .copied()
        .filter(|offset| *offset > 0.0)
        .max_by(f64::total_cmp);
    match (lower, upper) {
        (Some(lower), Some(upper)) => Some(ExtrusionSpan { lower, upper }),
        (Some(lower), None) => Some(ExtrusionSpan { lower, upper: 0.0 }),
        (None, Some(upper)) => Some(ExtrusionSpan { lower: 0.0, upper }),
        (None, None) => None,
    }
}

pub fn extrusion_extent_and_direction(
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let span = extrusion_span(profile_origin, direction, planes)?;
    let direction = normalized(direction)?;
    if span.lower == 0.0 || span.upper == 0.0 {
        let signed_length = if span.upper == 0.0 {
            span.lower
        } else {
            span.upper
        };
        return Some((
            ExtrudeExtent::OneSided {
                side: blind_extrude_side(signed_length.abs()),
            },
            direction.map(|value| value * signed_length.signum()),
        ));
    }
    let first = span.upper;
    let second = -span.lower;
    let scale = first.max(second).max(1.0);
    let extent = if (first - second).abs() <= EPS_EXTENT_AGREEMENT * scale {
        ExtrudeExtent::Symmetric {
            side: blind_extrude_side(first + second),
        }
    } else {
        ExtrudeExtent::TwoSided {
            first: blind_extrude_side(first),
            second: blind_extrude_side(second),
        }
    };
    Some((extent, direction))
}

pub fn blind_extrude_side(length: f64) -> ExtrudeSide {
    ExtrudeSide {
        termination: LinearTermination::Blind {
            length: Length(length),
        },
        draft: None,
    }
}

#[cfg(test)]
mod tests;
