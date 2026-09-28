// SPDX-License-Identifier: Apache-2.0
//! Compact hole and circular-sweep geometry.

use crate::vecmath::normalize;
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{
    BooleanOp, ExtrudeExtent, ExtrudeSide, FeatureDefinition as IrFeatureDefinition,
    FeatureOperation as IrFeatureOperation, LinearTermination, ProfileRef,
};
use cadmpeg_ir::geometry::analytic::CylinderSurface;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::container::ContainerScan;

use super::super::sweep::planes::{
    feature_outline_plane, feature_outline_planes, FeatureOutlinePlane,
};
use super::super::uniqueness::exactly_one;
use super::placement::{
    cap_square_center_radius, cylinder_from_single_cap_outline, hole_cylinder_from_cap_outlines,
    hole_placement, plane_envelope_corners, CapOutline, ExtrusionSpan, SimpleHoleGeometry,
};

const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;
const EPS_CENTER_AGREEMENT: f64 = 1.0e-9;
const EPS_OFFSET_NONZERO: f64 = 1.0e-12;
const EPS_EXTENT_AGREEMENT: f64 = 1.0e-9;

pub(in crate::decode) fn simple_hole_geometry<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<SimpleHoleGeometry<'a>>, CodecError> {
    let Some(cap_rows) = feature_outline_planes(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let candidate = (|| {
        let [first, second] = cap_rows.as_slice() else {
            return None;
        };
        let cap = |(id, origin, normal): FeatureOutlinePlane| {
            let envelope = exactly_one(scan.planes.envelopes.iter().filter(|envelope| envelope.surface_id == id))?;
            Some(CapOutline {
                surface_id: id,
                origin,
                normal,
                corners: plane_envelope_corners(&envelope.envelope)?,
            })
        };
        let first = cap(*first)?;
        let second = cap(*second)?;
        let table = exactly_one(scan.features.entity_tables.iter().filter(|table| {
            table.feature_id == feature_id && table.surface_ids_iter().next().is_some()
        }))?;
        let [entry_plane, termination_plane, first_cylinder, second_cylinder] = table.entries.as_slice()
        else {
            return None;
        };
        if entry_plane.entity_id != first.surface_id || termination_plane.entity_id != second.surface_id {
            return None;
        }
        let cylinder_row = |id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, id).filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
            })
        };
        let first_row = cylinder_row(first_cylinder.entity_id)?;
        let second_row = cylinder_row(second_cylinder.entity_id)?;
        let (_, _, extent) = hole_placement([first, second].map(|cap| (cap.surface_id, cap.origin, cap.normal)))?;
        let geometry = hole_cylinder_from_cap_outlines([first, second])?;
        Some((entry_plane.entity_id, first_row, second_row, extent, geometry))
    })();
    let Some((entry_surface_id, first_row, second_row, extent, geometry)) = candidate else {
        return Ok(None);
    };
    let mut cylinder_rows = Vec::new();
    ctx.try_reserve_items(&mut cylinder_rows, 2, "creo simple hole cylinder rows")?;
    cylinder_rows.extend([first_row, second_row]);
    Ok(Some(SimpleHoleGeometry {
        entry_surface_id: Some(entry_surface_id),
        cylinder_rows,
        extent,
        geometry,
    }))
}

fn has_exact_materialized_surface_roster(
    table: &crate::feature::entity::FeatureEntityTable,
    expected_ids: impl IntoIterator<Item = u32> + Clone,
) -> bool {
    let expected_count = expected_ids.clone().into_iter().count();
    if table.surface_ids_iter().count() != expected_count
        || table.unique_surface_ids().len() != expected_count
    {
        return false;
    }
    expected_ids.clone().into_iter().enumerate().all(|(index, id)| {
        table.unique_surface_ids().contains(&id)
            && !expected_ids.clone().into_iter().take(index).any(|previous| previous == id)
    })
}

pub(in crate::decode) fn compact_simple_hole_cylinder_id(
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<u32> {
    exactly_one(
        tables
        .iter()
        .filter(|table| table.feature_id == feature_id && table.table_class_id == 29)
        .filter_map(|table| {
            let topology_candidates = table
                .entries
                .windows(2)
                .enumerate()
                .filter_map(|(index, pair)| {
                    let [class_204, class_203] = pair.first_chunk::<2>()?;
                    (class_204.class_id() == 204
                        && class_203.class_id() == 203
                        && class_204.source_entity_id().is_none()
                        && class_203.source_entity_id().is_none())
                    .then_some(())?;
                    let mut planes = pair
                        .iter()
                        .filter(|candidate| {
                            table.contains_surface_id(candidate.entity_id)
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
                        });
                    let plane = match (planes.next(), planes.next()) {
                        (None, None) if table.entries.len() == 4 => None,
                        (Some(plane), None) => Some(plane.entity_id),
                        _ => return None,
                    };
                    pair.iter()
                        .filter(|candidate| Some(candidate.entity_id) != plane)
                        .all(|candidate| {
                            !table.contains_surface_id(candidate.entity_id)
                                && !rows.iter().any(|row| row.id == candidate.entity_id)
                        })
                        .then_some((index, plane))
                });
            let (topology_index, plane) = exactly_one(topology_candidates)?;
            let bottoms = table
                .entries
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate.source_entity_id() == Some(0)
                        && !table.contains_surface_id(candidate.entity_id)
                        && !rows.iter().any(|row| row.id == candidate.entity_id)
                });
            let (bottom_index, _) = exactly_one(bottoms)?;
            let sides = table
                .entries
                .iter()
                .enumerate()
                .filter(|(_, candidate)| {
                    candidate.class_id() == 200
                        && candidate.source_entity_id().is_none()
                        && table.contains_surface_id(candidate.entity_id)
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
                });
            let (side_index, side) = exactly_one(sides)?;
            let roster_matches = match plane {
                Some(plane_id) if plane_id != side.entity_id => {
                    has_exact_materialized_surface_roster(table, [side.entity_id, plane_id])
                }
                _ => has_exact_materialized_surface_roster(table, [side.entity_id]),
            };
            (roster_matches
                && topology_index < bottom_index
                && bottom_index < side_index)
                .then_some(side.entity_id)
        }),
    )
}

pub(in crate::decode) fn compact_simple_hole_geometry<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<SimpleHoleGeometry<'a>>, CodecError> {
    let candidate = (|| {
        let cylinder_id = compact_simple_hole_cylinder_id(
        feature_id,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
        )?;
        let frame = crate::surface::unique_surface_parameter(&scan.surfaces.parameters, cylinder_id)?
            .positional_cylinder_frame()?;
        let length = frame.length()?;
        let row = crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_id)?;
        Some((frame, length, row))
    })();
    let Some((frame, length, row)) = candidate else {
        return Ok(None);
    };
    let mut cylinder_rows = Vec::new();
    ctx.try_reserve_items(&mut cylinder_rows, 1, "creo compact hole cylinder rows")?;
    cylinder_rows.push(row);
    Ok(Some(SimpleHoleGeometry {
        entry_surface_id: None,
        cylinder_rows,
        extent: LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::from(length),
        },
        geometry: CylinderSurface::new(
            frame.frame().finite_origin(),
            frame.frame().orthonormal_frame(),
            frame.radius(),
        ),
    }))
}

pub(in crate::decode) fn circular_sweep_cylinder_from_cap_outlines(
    planes: [FeatureOutlinePlane; 2],
    outlines: impl IntoIterator<Item = CapOutline>,
) -> Option<CylinderSurface> {
    let (_, axis, _) = hole_placement(planes)?;
    let aligned_axis = super::placement::axis_aligned_with(axis, EPS_AXIS_ALIGNMENT)?;
    let radial = aligned_axis
        .complement()
        .map(crate::decode::axis::Axis::index);
    let mut circles = outlines
        .into_iter()
        .filter_map(|cap| cap_square_center_radius(cap.corners, aligned_axis));
    let (center, radius) = circles.next()?;
    let scale = center
        .iter()
        .chain(std::iter::once(&radius))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if circles.any(|(other_center, other_radius)| {
        radial.iter().any(|index| {
            (center[*index] - other_center[*index]).abs() > EPS_CENTER_AGREEMENT * scale
        }) || (radius - other_radius).abs() > EPS_CENTER_AGREEMENT * scale
    }) {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    CylinderSurface::try_new(
        Point3::from(center),
        Vector3::from(axis),
        Vector3::from(ref_direction),
        radius,
    )
    .ok()
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::decode) struct CircularSweepGeometry<'a> {
    pub(in crate::decode) cylinder_rows: Vec<&'a crate::surface::SurfaceRow>,
    pub(in crate::decode) section_definition_id: Option<u32>,
    pub(in crate::decode) direction: [f64; 3],
    pub(in crate::decode) extent: ExtrudeExtent,
    pub(in crate::decode) geometry: CylinderSurface,
}

pub(in crate::decode) fn single_cap_circular_sweep_geometry<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<CircularSweepGeometry<'a>> {
    let table = exactly_one(scan.features.entity_tables.iter().filter(|table| {
        table.feature_id == feature_id && table.surface_ids_iter().next().is_some()
    }))?;
    let [first_cap, second_cap, profile_id, cylinder_id] = table.entries.as_slice() else {
        return None;
    };
    let (rowless_cap, cap_id) = match (
        table.contains_surface_id(first_cap.entity_id),
        table.contains_surface_id(second_cap.entity_id),
    ) {
        (true, false) => (second_cap, first_cap),
        (false, true) => (first_cap, second_cap),
        _ => return None,
    };
    if [
        first_cap.class_id(),
        second_cap.class_id(),
        profile_id.class_id(),
        cylinder_id.class_id(),
    ] != [204, 203, 200, 200]
        || profile_id.source_entity_id().is_none()
        || cylinder_id.source_entity_id().is_some()
        || !has_exact_materialized_surface_roster(table, [cap_id.entity_id, cylinder_id.entity_id])
        || !table.contains_non_surface_entity_id(rowless_cap.entity_id)
        || !table.contains_non_surface_entity_id(profile_id.entity_id)
    {
        return None;
    }
    crate::surface::unique_surface_row(&scan.surfaces.rows, cap_id.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .then_some(())?;
    let cylinder_row =
        crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_id.entity_id).filter(
            |row| row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder,
        )?;
    let plane = feature_outline_plane(scan, feature_id, cap_id.entity_id)?;
    let envelope = exactly_one(scan.planes.envelopes.iter().filter(|envelope| envelope.surface_id == cap_id.entity_id))?;
    let cap = CapOutline {
        surface_id: plane.0,
        origin: plane.1,
        normal: plane.2,
        corners: plane_envelope_corners(&envelope.envelope)?,
    };
    let transform = exactly_one(scan.features.section_transforms.iter().filter(|transform| transform.feature_id == Some(feature_id)))?;
    let (extent, direction) = extrusion_extent_and_direction(
        transform.origin(),
        transform.normal(),
        [(plane.1, plane.2)],
    )?;
    Some(CircularSweepGeometry {
        cylinder_rows: vec![cylinder_row],
        section_definition_id: Some(transform.definition_id),
        direction,
        extent,
        geometry: cylinder_from_single_cap_outline(cap)?,
    })
}

pub(in crate::decode) fn circular_sweep_feature_definition(
    profile: ProfileRef,
    sweep: &CircularSweepGeometry<'_>,
    op: BooleanOp,
    solid: Option<bool>,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Operation(IrFeatureOperation::Extrude {
        profile,
        direction: cadmpeg_ir::features::FeatureDirection3::new(Vector3::from(sweep.direction))
            .map_or(
                cadmpeg_ir::features::ExtrudeDirection::Unresolved {},
                |vector| cadmpeg_ir::features::ExtrudeDirection::Explicit {
                    vector,
                    source: None,
                },
            ),
        start: cadmpeg_ir::features::ExtrudeStart::default(),
        extent: sweep.extent.clone(),
        op,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    })
}

pub(in crate::decode) fn circular_sweep_geometry<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<CircularSweepGeometry<'a>> {
    two_cap_circular_sweep_geometry(scan, feature_id)
        .or_else(|| single_cap_circular_sweep_geometry(scan, feature_id))
}

pub(in crate::decode) fn two_cap_circular_sweep_geometry<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<CircularSweepGeometry<'a>> {
    let table = exactly_one(scan.features.entity_tables.iter().filter(|table| {
        table.feature_id == feature_id && table.surface_ids_iter().next().is_some()
    }))?;
    let [first_plane_entry, second_plane_entry, profile_entry, cylinder_entry] =
        table.entries.as_slice()
    else {
        return None;
    };
    if [
            first_plane_entry.class_id(),
            second_plane_entry.class_id(),
            profile_entry.class_id(),
            cylinder_entry.class_id(),
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
        || table.contains_surface_id(profile_entry.entity_id)
        || !table.contains_non_surface_entity_id(profile_entry.entity_id)
    {
        return None;
    }
    let first = feature_outline_plane(scan, feature_id, first_plane_entry.entity_id)?;
    let second = feature_outline_plane(scan, feature_id, second_plane_entry.entity_id)?;
    let cap = |plane: FeatureOutlinePlane| {
        let corners = exactly_one(scan.planes.envelopes.iter().filter(|envelope| envelope.surface_id == plane.0))
            .and_then(|envelope| plane_envelope_corners(&envelope.envelope));
        Some(CapOutline {
            surface_id: plane.0,
            origin: plane.1,
            normal: plane.2,
            corners: corners?,
        })
    };
    let cylinder_row =
        crate::surface::unique_surface_row(&scan.surfaces.rows, cylinder_entry.entity_id).filter(
            |row| row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder,
        )?;
    let (_, direction, termination) = hole_placement([first, second])?;
    Some(CircularSweepGeometry {
        cylinder_rows: vec![cylinder_row],
        section_definition_id: None,
        direction,
        extent: ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination,
                draft: None,
            },
        },
        geometry: circular_sweep_cylinder_from_cap_outlines(
            [first, second],
            [cap(first), cap(second)].into_iter().flatten(),
        )?,
    })
}

pub(in crate::decode) fn extrusion_span(
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
        (Some(lower), Some(upper)) => ExtrusionSpan::new(lower, upper),
        (Some(lower), None) => ExtrusionSpan::new(lower, 0.0),
        (None, Some(upper)) => ExtrusionSpan::new(0.0, upper),
        (None, None) => None,
    }
}

pub(in crate::decode) fn extrusion_extent_and_direction(
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let span = extrusion_span(profile_origin, direction, planes)?;
    let direction = normalize(direction)?;
    if span.lower() == 0.0 || span.upper() == 0.0 {
        let signed_length = if span.upper() == 0.0 {
            span.lower()
        } else {
            span.upper()
        };
        return Some((
            ExtrudeExtent::OneSided {
                side: blind_extrude_side(signed_length.abs())?,
            },
            direction.map(|value| value * signed_length.signum()),
        ));
    }
    let first = span.upper();
    let second = -span.lower();
    let scale = first.max(second).max(1.0);
    let extent = if (first - second).abs() <= EPS_EXTENT_AGREEMENT * scale {
        ExtrudeExtent::Symmetric {
            side: blind_extrude_side(first + second)?,
        }
    } else {
        ExtrudeExtent::TwoSided {
            first: blind_extrude_side(first)?,
            second: blind_extrude_side(second)?,
        }
    };
    Some((extent, direction))
}

pub(in crate::decode) fn blind_extrude_side(length: f64) -> Option<ExtrudeSide> {
    Some(ExtrudeSide {
        termination: LinearTermination::Blind {
            length: cadmpeg_ir::scalar::NonZeroLength::new(length)?,
        },
        draft: None,
    })
}

#[cfg(test)]
mod tests;
