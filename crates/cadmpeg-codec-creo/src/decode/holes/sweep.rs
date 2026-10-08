// SPDX-License-Identifier: Apache-2.0
//! Compact hole and circular-sweep geometry.

use crate::feature::entity::FeatureEntityTableEntry;
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
use super::super::uniqueness::exactly_one_by;
use super::placement::{
    cap_square_center_radius, cylinder_from_single_cap_outline, hole_cylinder_from_cap_outlines,
    hole_placement, plane_envelope_corners, CapOutline, ExtrusionSpan, HoleCylinderRows,
    SimpleHoleGeometry,
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
    let Some((cap_rows, _cap_storage)) = feature_outline_planes(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let [first, second] = cap_rows.as_slice() else {
        return Ok(None);
    };
    let Some(first) = cap_outline(ctx, scan, *first)? else {
        return Ok(None);
    };
    let Some(second) = cap_outline(ctx, scan, *second)? else {
        return Ok(None);
    };
    let Some(table) = materialized_feature_table(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let [entry_plane, termination_plane, first_cylinder, second_cylinder] =
        table.entries.as_slice()
    else {
        return Ok(None);
    };
    if entry_plane.entity_id != first.surface_id || termination_plane.entity_id != second.surface_id
    {
        return Ok(None);
    }
    let cylinder_row = |id| {
        scan.surfaces.rows.unique(id).filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    };
    let Some(first_row) = cylinder_row(first_cylinder.entity_id) else {
        return Ok(None);
    };
    let Some(second_row) = cylinder_row(second_cylinder.entity_id) else {
        return Ok(None);
    };
    let Some((_, _, extent)) =
        hole_placement([first, second].map(|cap| (cap.surface_id, cap.origin, cap.normal)))
    else {
        return Ok(None);
    };
    let Some(geometry) = hole_cylinder_from_cap_outlines([first, second]) else {
        return Ok(None);
    };
    let entry_surface_id = entry_plane.entity_id;
    Ok(Some(SimpleHoleGeometry {
        entry_surface_id: Some(entry_surface_id),
        cylinder_rows: HoleCylinderRows::Two([first_row, second_row]),
        extent,
        geometry,
    }))
}

fn materialized_feature_table<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<&'a crate::feature::entity::FeatureEntityTable>, CodecError> {
    exactly_one_by(
        ctx,
        &scan.features.entity_tables,
        |table| Ok(table.feature_id == feature_id && !table.unique_surface_ids().is_empty()),
        "creo hole feature table scan",
    )
}

fn cap_outline(
    ctx: &DecodeContext<'_>,
    scan: &ContainerScan<'_>,
    plane: FeatureOutlinePlane,
) -> Result<Option<CapOutline>, CodecError> {
    let Some(envelope) = exactly_one_by(
        ctx,
        &scan.planes.envelopes,
        |envelope| Ok(envelope.surface_id == plane.0),
        "creo hole cap envelope scan",
    )?
    else {
        return Ok(None);
    };
    let Some(corners) = plane_envelope_corners(&envelope.envelope) else {
        return Ok(None);
    };
    Ok(Some(CapOutline {
        surface_id: plane.0,
        origin: plane.1,
        normal: plane.2,
        corners,
    }))
}

fn has_exact_materialized_surface_roster<const N: usize>(
    ctx: &DecodeContext<'_>,
    table: &crate::feature::entity::FeatureEntityTable,
    expected_ids: &[u32; N],
) -> Result<bool, CodecError> {
    let expected_count = expected_ids.len();
    if table.unique_surface_ids().len() != expected_count {
        return Ok(false);
    }
    let mut actual_count = 0;
    let mut entries = table.entries.iter();
    while let Some(entry) =
        ctx.next_charged(&mut entries, "creo materialized surface roster count")?
    {
        if ctx.contains_btree_set(
            table.unique_surface_ids(),
            &entry.entity_id,
            "creo surface roster membership",
        )? {
            actual_count += 1;
            if actual_count > expected_count {
                return Ok(false);
            }
        }
    }
    if actual_count != expected_count {
        return Ok(false);
    }
    for (index, id) in expected_ids.iter().enumerate() {
        if !ctx.contains_btree_set(
            table.unique_surface_ids(),
            id,
            "creo surface roster membership",
        )? || expected_ids[..index].contains(id)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(in crate::decode) fn compact_simple_hole_cylinder_id(
    ctx: &DecodeContext<'_>,
    feature_id: u32,
    tables: &[crate::feature::entity::FeatureEntityTable],
    rows: &crate::surface::SurfaceRows,
) -> Result<Option<u32>, CodecError> {
    let mut first = None;
    let mut tables = tables.iter();
    'tables: while let Some(table) = ctx.next_charged(&mut tables, "creo compact hole tables")? {
        if table.feature_id != feature_id || table.table_class_id != 29 {
            continue;
        }
        let mut topology = None;
        let mut pairs = table.entries.windows(2).enumerate();
        while let Some((index, pair)) =
            ctx.next_charged(&mut pairs, "creo compact hole topology")?
        {
            if pair[0].class_id() != 204
                || pair[1].class_id() != 203
                || pair[0].source_entity_id().is_some()
                || pair[1].source_entity_id().is_some()
            {
                continue;
            }
            let mut plane = None;
            let mut ambiguous_plane = false;
            for candidate in pair {
                if ctx.contains_btree_set(
                    table.unique_surface_ids(),
                    &candidate.entity_id,
                    "creo surface roster membership",
                )? && rows.unique(candidate.entity_id).is_some_and(|row| {
                    row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
                }) {
                    if plane.is_some() {
                        ambiguous_plane = true;
                        break;
                    }
                    plane = Some(candidate.entity_id);
                }
            }
            if ambiguous_plane || (plane.is_none() && table.entries.len() != 4) {
                continue;
            }
            let mut rowless = true;
            for candidate in pair {
                if Some(candidate.entity_id) != plane
                    && (ctx.contains_btree_set(
                        table.unique_surface_ids(),
                        &candidate.entity_id,
                        "creo surface roster membership",
                    )? || rows.contains_id(candidate.entity_id))
                {
                    rowless = false;
                    break;
                }
            }
            if !rowless {
                continue;
            }
            if topology.is_some() {
                continue 'tables;
            }
            topology = Some((index, plane));
        }
        let Some((topology_index, plane)) = topology else {
            continue;
        };
        let bottom_matches = |candidate: &FeatureEntityTableEntry| -> Result<bool, CodecError> {
            if candidate.source_entity_id() != Some(0) {
                return Ok(false);
            }
            Ok(!ctx.contains_btree_set(
                table.unique_surface_ids(),
                &candidate.entity_id,
                "creo surface roster membership",
            )? && !rows.contains_id(candidate.entity_id))
        };
        let Some(bottom_index) = ctx.position_by(
            &table.entries,
            bottom_matches,
            "creo compact hole bottom scan",
        )?
        else {
            continue;
        };
        if ctx.any_by(
            &table.entries[bottom_index + 1..],
            bottom_matches,
            "creo compact hole bottom scan",
        )? {
            continue;
        }
        let mut side = None;
        let mut entries = table.entries.iter().enumerate();
        while let Some((index, candidate)) =
            ctx.next_charged(&mut entries, "creo compact hole side scan")?
        {
            if candidate.class_id() == 200
                && candidate.source_entity_id().is_none()
                && ctx.contains_btree_set(
                    table.unique_surface_ids(),
                    &candidate.entity_id,
                    "creo surface roster membership",
                )?
                && rows.unique(candidate.entity_id).is_some_and(|row| {
                    row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                })
            {
                if side.is_some() {
                    continue 'tables;
                }
                side = Some((index, candidate));
            }
        }
        let Some((side_index, side)) = side else {
            continue;
        };
        let roster_matches = match plane {
            Some(plane_id) if plane_id != side.entity_id => {
                has_exact_materialized_surface_roster(ctx, table, &[side.entity_id, plane_id])?
            }
            _ => has_exact_materialized_surface_roster(ctx, table, &[side.entity_id])?,
        };
        if roster_matches && topology_index < bottom_index && bottom_index < side_index {
            if first.is_some() {
                return Ok(None);
            }
            first = Some(side.entity_id);
        }
    }
    Ok(first)
}

pub(in crate::decode) fn compact_simple_hole_geometry<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<SimpleHoleGeometry<'a>>, CodecError> {
    let Some(cylinder_id) = compact_simple_hole_cylinder_id(
        ctx,
        feature_id,
        &scan.features.entity_tables,
        &scan.surfaces.rows,
    )?
    else {
        return Ok(None);
    };
    let Some(parameter) = scan.surfaces.parameters.unique(cylinder_id) else {
        return Ok(None);
    };
    let Some(frame) = parameter.positional_cylinder_frame() else {
        return Ok(None);
    };
    let Some(length) = frame.length() else {
        return Ok(None);
    };
    let Some(row) = scan.surfaces.rows.unique(cylinder_id) else {
        return Ok(None);
    };
    Ok(Some(SimpleHoleGeometry {
        entry_surface_id: None,
        cylinder_rows: HoleCylinderRows::One([row]),
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
    ctx: &DecodeContext<'_>,
    planes: [FeatureOutlinePlane; 2],
    outlines: impl IntoIterator<Item = CapOutline>,
) -> Result<Option<CylinderSurface>, CodecError> {
    let Some((_, axis, _)) = hole_placement(planes) else {
        return Ok(None);
    };
    let Some(aligned_axis) = super::placement::axis_aligned_with(axis, EPS_AXIS_ALIGNMENT) else {
        return Ok(None);
    };
    let radial = aligned_axis.complement().map(crate::axis::Axis::index);
    let mut outlines = outlines.into_iter();
    let Some((center, radius)) = ctx.find_map(
        &mut outlines,
        |cap| Ok(cap_square_center_radius(cap.corners, aligned_axis)),
        "creo circular sweep cap scan",
    )?
    else {
        return Ok(None);
    };
    let scale = center
        .iter()
        .chain(std::iter::once(&radius))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if ctx.any_by(
        outlines,
        |cap| {
            Ok(
                cap_square_center_radius(cap.corners, aligned_axis).is_some_and(
                    |(other_center, other_radius)| {
                        radial.iter().any(|index| {
                            (center[*index] - other_center[*index]).abs()
                                > EPS_CENTER_AGREEMENT * scale
                        }) || (radius - other_radius).abs() > EPS_CENTER_AGREEMENT * scale
                    },
                ),
            )
        },
        "creo circular sweep cap scan",
    )? {
        return Ok(None);
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Ok(CylinderSurface::try_new(
        Point3::from(center),
        Vector3::from(axis),
        Vector3::from(ref_direction),
        radius,
    )
    .ok())
}

#[derive(Debug, Clone, PartialEq)]
pub(in crate::decode) struct CircularSweepGeometry<'a> {
    pub(in crate::decode) cylinder_rows: HoleCylinderRows<'a>,
    pub(in crate::decode) section_definition_id: Option<u32>,
    pub(in crate::decode) direction: [f64; 3],
    pub(in crate::decode) extent: ExtrudeExtent,
    pub(in crate::decode) geometry: CylinderSurface,
}

pub(in crate::decode) fn single_cap_circular_sweep_geometry<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<CircularSweepGeometry<'a>>, CodecError> {
    let Some(table) = materialized_feature_table(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let [first_cap, second_cap, profile_id, cylinder_id] = table.entries.as_slice() else {
        return Ok(None);
    };
    let (rowless_cap, cap_id) = match (
        ctx.contains_btree_set(
            table.unique_surface_ids(),
            &first_cap.entity_id,
            "creo surface roster membership",
        )?,
        ctx.contains_btree_set(
            table.unique_surface_ids(),
            &second_cap.entity_id,
            "creo surface roster membership",
        )?,
    ) {
        (true, false) => (second_cap, first_cap),
        (false, true) => (first_cap, second_cap),
        _ => return Ok(None),
    };
    if [
        first_cap.class_id(),
        second_cap.class_id(),
        profile_id.class_id(),
        cylinder_id.class_id(),
    ] != [204, 203, 200, 200]
        || profile_id.source_entity_id().is_none()
        || cylinder_id.source_entity_id().is_some()
    {
        return Ok(None);
    }
    if !has_exact_materialized_surface_roster(
        ctx,
        table,
        &[cap_id.entity_id, cylinder_id.entity_id],
    )? {
        return Ok(None);
    }
    if ctx.contains_btree_set(
        table.unique_surface_ids(),
        &rowless_cap.entity_id,
        "creo surface roster membership",
    )? || ctx.contains_btree_set(
        table.unique_surface_ids(),
        &profile_id.entity_id,
        "creo surface roster membership",
    )? {
        return Ok(None);
    }
    if !scan
        .surfaces
        .rows
        .unique(cap_id.entity_id)
        .is_some_and(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
    {
        return Ok(None);
    }
    let Some(cylinder_row) = scan
        .surfaces
        .rows
        .unique(cylinder_id.entity_id)
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    else {
        return Ok(None);
    };
    let Some(plane) = feature_outline_plane(ctx, scan, feature_id, cap_id.entity_id)? else {
        return Ok(None);
    };
    let Some(cap) = cap_outline(ctx, scan, plane)? else {
        return Ok(None);
    };
    let Some(transform) = exactly_one_by(
        ctx,
        &scan.features.section_transforms,
        |transform| Ok(transform.feature_id == Some(feature_id)),
        "creo single cap transform scan",
    )?
    else {
        return Ok(None);
    };
    let Some((extent, direction)) = extrusion_extent_and_direction(
        ctx,
        transform.origin(),
        transform.normal(),
        [(plane.1, plane.2)],
    )?
    else {
        return Ok(None);
    };
    let Some(geometry) = cylinder_from_single_cap_outline(cap) else {
        return Ok(None);
    };
    let definition_id = transform.definition_id;
    Ok(Some(CircularSweepGeometry {
        cylinder_rows: HoleCylinderRows::One([cylinder_row]),
        section_definition_id: Some(definition_id),
        direction,
        extent,
        geometry,
    }))
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
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<CircularSweepGeometry<'a>>, CodecError> {
    let two_cap = two_cap_circular_sweep_geometry(ctx, scan, feature_id)?;
    if two_cap.is_some() {
        Ok(two_cap)
    } else {
        single_cap_circular_sweep_geometry(ctx, scan, feature_id)
    }
}

pub(in crate::decode) fn two_cap_circular_sweep_geometry<'a>(
    ctx: &DecodeContext<'_>,
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Result<Option<CircularSweepGeometry<'a>>, CodecError> {
    let Some(table) = materialized_feature_table(ctx, scan, feature_id)? else {
        return Ok(None);
    };
    let [first_plane_entry, second_plane_entry, profile_entry, cylinder_entry] =
        table.entries.as_slice()
    else {
        return Ok(None);
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
    {
        return Ok(None);
    }
    if !has_exact_materialized_surface_roster(
        ctx,
        table,
        &[
            first_plane_entry.entity_id,
            second_plane_entry.entity_id,
            cylinder_entry.entity_id,
        ],
    )? || ctx.contains_btree_set(
        table.unique_surface_ids(),
        &profile_entry.entity_id,
        "creo surface roster membership",
    )? {
        return Ok(None);
    }
    let Some(first) = feature_outline_plane(ctx, scan, feature_id, first_plane_entry.entity_id)?
    else {
        return Ok(None);
    };
    let Some(second) = feature_outline_plane(ctx, scan, feature_id, second_plane_entry.entity_id)?
    else {
        return Ok(None);
    };
    let Some(cylinder_row) = scan
        .surfaces
        .rows
        .unique(cylinder_entry.entity_id)
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
        })
    else {
        return Ok(None);
    };
    let Some((_, direction, termination)) = hole_placement([first, second]) else {
        return Ok(None);
    };
    let extent = ExtrudeExtent::OneSided {
        side: ExtrudeSide {
            termination,
            draft: None,
        },
    };
    let caps = [
        cap_outline(ctx, scan, first)?,
        cap_outline(ctx, scan, second)?,
    ];
    let Some(geometry) = circular_sweep_cylinder_from_cap_outlines(
        ctx,
        [first, second],
        caps.into_iter().flatten(),
    )?
    else {
        return Ok(None);
    };
    Ok(Some(CircularSweepGeometry {
        cylinder_rows: HoleCylinderRows::One([cylinder_row]),
        section_definition_id: None,
        direction,
        extent,
        geometry,
    }))
}

pub(in crate::decode) fn extrusion_span(
    ctx: &DecodeContext<'_>,
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Result<Option<ExtrusionSpan>, CodecError> {
    let direction_length = direction
        .iter()
        .map(|value| value * value)
        .sum::<f64>()
        .sqrt();
    if direction_length <= f64::EPSILON {
        return Ok(None);
    }
    let direction = direction.map(|value| value / direction_length);
    let mut smallest_positive: Option<f64> = None;
    let mut largest_positive: Option<f64> = None;
    let mut smallest_negative: Option<f64> = None;
    let mut largest_negative: Option<f64> = None;
    let mut planes = planes.into_iter();
    while let Some((origin, normal)) =
        ctx.next_charged(&mut planes, "creo extrusion plane span scan")?
    {
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
        let duplicate = [
            smallest_positive,
            largest_positive,
            smallest_negative,
            largest_negative,
        ]
        .into_iter()
        .flatten()
        .any(|known| (known - offset).abs() <= EPS_EXTENT_AGREEMENT * scale);
        if duplicate {
            continue;
        }
        if offset > 0.0 {
            smallest_positive = Some(smallest_positive.map_or(offset, |known| known.min(offset)));
            largest_positive = Some(largest_positive.map_or(offset, |known| known.max(offset)));
        } else if offset < 0.0 {
            smallest_negative = Some(smallest_negative.map_or(offset, |known| known.min(offset)));
            largest_negative = Some(largest_negative.map_or(offset, |known| known.max(offset)));
        }
    }
    let lower = smallest_negative;
    let upper = largest_positive;
    Ok(match (lower, upper) {
        (Some(lower), Some(upper)) => ExtrusionSpan::new(lower, upper),
        (Some(lower), None) => ExtrusionSpan::new(lower, 0.0),
        (None, Some(upper)) => ExtrusionSpan::new(0.0, upper),
        (None, None) => None,
    })
}

pub(in crate::decode) fn extrusion_extent_and_direction(
    ctx: &DecodeContext<'_>,
    profile_origin: [f64; 3],
    direction: [f64; 3],
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Result<Option<(ExtrudeExtent, [f64; 3])>, CodecError> {
    let Some(span) = extrusion_span(ctx, profile_origin, direction, planes)? else {
        return Ok(None);
    };
    let Some(direction) = normalize(direction) else {
        return Ok(None);
    };
    if span.lower() == 0.0 || span.upper() == 0.0 {
        let signed_length = if span.upper() == 0.0 {
            span.lower()
        } else {
            span.upper()
        };
        return Ok(Some((
            ExtrudeExtent::OneSided {
                side: match blind_extrude_side(signed_length.abs()) {
                    Some(side) => side,
                    None => return Ok(None),
                },
            },
            direction.map(|value| value * signed_length.signum()),
        )));
    }
    let first = span.upper();
    let second = -span.lower();
    let scale = first.max(second).max(1.0);
    let extent = if (first - second).abs() <= EPS_EXTENT_AGREEMENT * scale {
        ExtrudeExtent::Symmetric {
            side: match blind_extrude_side(first + second) {
                Some(side) => side,
                None => return Ok(None),
            },
        }
    } else {
        ExtrudeExtent::TwoSided {
            first: match blind_extrude_side(first) {
                Some(side) => side,
                None => return Ok(None),
            },
            second: match blind_extrude_side(second) {
                Some(side) => side,
                None => return Ok(None),
            },
        }
    };
    Ok(Some((extent, direction)))
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
