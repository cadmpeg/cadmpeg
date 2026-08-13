// SPDX-License-Identifier: Apache-2.0
//! Hole and circular-sweep construction from outlines, envelopes, and cap pairs.

#[allow(clippy::wildcard_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct ExtrusionSpan {
    pub(super) lower: f64,
    pub(super) upper: f64,
}

pub(super) fn hole_extent_and_direction(
    planes: impl IntoIterator<Item = ([f64; 3], [f64; 3])>,
) -> Option<([f64; 3], Termination)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(first_origin, first_normal), (second_origin, second_normal)] = planes.as_slice() else {
        return None;
    };
    let first_normal = normalized(*first_normal)?;
    let second_normal = normalized(*second_normal)?;
    let alignment = first_normal
        .iter()
        .zip(second_normal)
        .map(|(first, second)| first * second)
        .sum::<f64>()
        .abs();
    if (alignment - 1.0).abs() > 1e-9 {
        return None;
    }
    let signed_length = second_origin
        .iter()
        .zip(first_origin)
        .zip(first_normal)
        .map(|((second, first), axis)| (second - first) * axis)
        .sum::<f64>();
    let scale = second_origin
        .iter()
        .chain(first_origin)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if signed_length.abs() <= 1e-9 * scale {
        return None;
    }
    Some((
        first_normal.map(|value| value * signed_length.signum()),
        Termination::Blind {
            length: Length(signed_length.abs()),
        },
    ))
}

pub(super) fn hole_placement(
    planes: impl IntoIterator<Item = (u32, [f64; 3], [f64; 3])>,
) -> Option<(u32, [f64; 3], Termination)> {
    let planes = planes.into_iter().collect::<Vec<_>>();
    let [(entry_id, entry_origin, entry_normal), (_, termination_origin, termination_normal)] =
        planes.as_slice()
    else {
        return None;
    };
    let (direction, extent) = hole_extent_and_direction([
        (*entry_origin, *entry_normal),
        (*termination_origin, *termination_normal),
    ])?;
    Some((*entry_id, direction, extent))
}

pub(super) fn plane_envelope_corners(
    envelope: &crate::surface::PlaneEnvelope,
) -> Option<[[f64; 3]; 2]> {
    let corners = match envelope {
        crate::surface::PlaneEnvelope::Standard { corners_3d, .. }
        | crate::surface::PlaneEnvelope::Compact { corners_3d, .. } => corners_3d,
    };
    Some([
        [corners[0][0]?, corners[0][1]?, corners[0][2]?],
        [corners[1][0]?, corners[1][1]?, corners[1][2]?],
    ])
}

pub(super) type HoleCapOutline = (u32, [f64; 3], [f64; 3], [[f64; 3]; 2]);
pub(super) type PartialCapOutline = (u32, [f64; 3], [f64; 3], Option<[[f64; 3]; 2]>);

pub(super) fn cap_square_center_radius(
    corners: [[f64; 3]; 2],
    axis_index: usize,
) -> Option<([f64; 3], f64)> {
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let spans = [
        (corners[1][radial[0]] - corners[0][radial[0]]).abs(),
        (corners[1][radial[1]] - corners[0][radial[1]]).abs(),
    ];
    let scale = spans[0]
        .max(spans[1])
        .max(corners[0][axis_index].abs())
        .max(corners[1][axis_index].abs())
        .max(1.0);
    if (corners[1][axis_index] - corners[0][axis_index]).abs() > 1e-9 * scale
        || spans[0] <= 1e-9
        || (spans[0] - spans[1]).abs() > 1e-9 * scale
    {
        return None;
    }
    Some((
        std::array::from_fn(|index| 0.5 * (corners[0][index] + corners[1][index])),
        0.5 * spans[0],
    ))
}

pub(super) fn cylinder_from_single_cap_outline(cap: PartialCapOutline) -> Option<SurfaceGeometry> {
    let (_, _, axis, corners) = cap;
    let axis = normalized(axis)?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let (center, radius) = cap_square_center_radius(corners?, axis_index)?;
    let radial_axis = (0..3).find(|index| *index != axis_index)?;
    let mut ref_direction = [0.0; 3];
    ref_direction[radial_axis] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius,
    })
}

pub(super) fn hole_cylinder_from_cap_outlines(
    caps: [HoleCapOutline; 2],
) -> Option<SurfaceGeometry> {
    let placement = hole_placement(caps.map(|(id, origin, normal, _)| (id, origin, normal)))?;
    let axis = placement.1;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let mut centers = Vec::<[f64; 3]>::new();
    let mut radii = Vec::new();
    for (_, _, _, corners) in caps {
        let (center, radius) = cap_square_center_radius(corners, axis_index)?;
        centers.push(center);
        radii.push(radius);
    }
    let scale = centers
        .iter()
        .flatten()
        .chain(&radii)
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    if radial
        .iter()
        .any(|index| (centers[0][*index] - centers[1][*index]).abs() > 1e-9 * scale)
        || (radii[0] - radii[1]).abs() > 1e-9 * scale
    {
        return None;
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(centers[0][0], centers[0][1], centers[0][2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius: radii[0],
    })
}

pub(super) fn cylinder_from_complementary_outline_bounds(
    plane: &SurfaceGeometry,
    bounds: [[[f64; 2]; 2]; 2],
) -> Option<SurfaceGeometry> {
    let SurfaceGeometry::Plane { origin, normal, .. } = plane else {
        return None;
    };
    let axis = normalized([normal.x, normal.y, normal.z])?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
    })?;
    let radial = (0..3)
        .filter(|index| *index != axis_index)
        .collect::<Vec<_>>();
    let scale = bounds
        .iter()
        .flatten()
        .flatten()
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    if bounds
        .iter()
        .any(|rectangle| (0..2).any(|index| rectangle[1][index] <= rectangle[0][index]))
    {
        return None;
    }
    let union = if close(bounds[0][0][0], bounds[1][0][0])
        && close(bounds[0][1][0], bounds[1][1][0])
        && (close(bounds[0][1][1], bounds[1][0][1]) || close(bounds[1][1][1], bounds[0][0][1]))
    {
        [
            [bounds[0][0][0], bounds[0][0][1].min(bounds[1][0][1])],
            [bounds[0][1][0], bounds[0][1][1].max(bounds[1][1][1])],
        ]
    } else if close(bounds[0][0][1], bounds[1][0][1])
        && close(bounds[0][1][1], bounds[1][1][1])
        && (close(bounds[0][1][0], bounds[1][0][0]) || close(bounds[1][1][0], bounds[0][0][0]))
    {
        [
            [bounds[0][0][0].min(bounds[1][0][0]), bounds[0][0][1]],
            [bounds[0][1][0].max(bounds[1][1][0]), bounds[0][1][1]],
        ]
    } else {
        return None;
    };
    let spans = [union[1][0] - union[0][0], union[1][1] - union[0][1]];
    if spans.iter().any(|span| !span.is_finite() || *span <= 0.0) || !close(spans[0], spans[1]) {
        return None;
    }
    let mut center = [origin.x, origin.y, origin.z];
    for (coordinate, index) in radial.iter().enumerate() {
        center[*index] = 0.5 * (union[0][coordinate] + union[1][coordinate]);
    }
    let mut ref_direction = [0.0; 3];
    ref_direction[radial[0]] = 1.0;
    Some(SurfaceGeometry::Cylinder {
        origin: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
        ref_direction: Vector3::new(ref_direction[0], ref_direction[1], ref_direction[2]),
        radius: 0.5 * spans[0],
    })
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct SimpleHoleGeometry {
    pub(super) entry_surface_id: Option<u32>,
    pub(super) cylinder_ids: Vec<u32>,
    pub(super) direction: [f64; 3],
    pub(super) extent: Termination,
    pub(super) geometry: SurfaceGeometry,
}

pub(super) fn stepped_hole_form(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<HoleForm> {
    let candidates = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .filter_map(|table| paired_hole_replay_surfaces_by_source(feature_id, table, rows))
        .filter(|generated_by_source| {
            let cylinder_sources = generated_by_source
                .values()
                .filter(|entries| {
                    matches!(
                        entries.as_slice(),
                        [
                            Some(crate::surface::SurfaceKind::Cylinder),
                            Some(crate::surface::SurfaceKind::Cylinder)
                        ]
                    )
                })
                .count();
            let planar_support_sources = generated_by_source
                .values()
                .filter(|entries| {
                    entries
                        .iter()
                        .filter(|kind| **kind == Some(crate::surface::SurfaceKind::Plane))
                        .count()
                        == 1
                        && entries.iter().filter(|kind| kind.is_none()).count() == 1
                })
                .count();
            let has_cone = generated_by_source
                .values()
                .flatten()
                .any(|kind| *kind == Some(crate::surface::SurfaceKind::Cone));
            cylinder_sources == 2 && planar_support_sources == 1 && !has_cone
        })
        .count();
    (candidates == 1).then_some(HoleForm::Counterbore)
}

pub(super) fn paired_hole_replay_surfaces_by_source(
    feature_id: u32,
    table: &crate::feature::FeatureEntityTable,
    rows: &[crate::surface::SurfaceRow],
) -> Option<BTreeMap<u32, [Option<crate::surface::SurfaceKind>; 2]>> {
    let entry_kind = |entry: &crate::feature::FeatureEntityTableEntry| {
        if table.surface_ids.contains(&entry.entity_id) {
            Some(Some(
                crate::surface::unique_surface_row(rows, entry.entity_id)
                    .filter(|row| row.feature_id == feature_id)?
                    .kind,
            ))
        } else {
            (table.non_surface_entity_ids.contains(&entry.entity_id)
                && !table.surface_ids.contains(&entry.entity_id))
            .then_some(None)
        }
    };
    let mut runs = Vec::<BTreeMap<u32, Option<crate::surface::SurfaceKind>>>::new();
    let mut framed_class_200_count = 0;
    let mut source_zero_count = 0;
    let mut index = 0;
    while index < table.entries.len() {
        let Some(class_203) = table.entries.get(index + 1) else {
            break;
        };
        let class_204 = &table.entries[index];
        if class_204.class_id != 204 || class_203.class_id != 203 {
            index += 1;
            continue;
        }
        entry_kind(class_204)?.is_none().then_some(())?;
        entry_kind(class_203)?.is_none().then_some(())?;
        index += 2;
        let mut run = BTreeMap::new();
        while let Some(entry) = table
            .entries
            .get(index)
            .filter(|entry| entry.class_id == 200)
        {
            framed_class_200_count += 1;
            let kind = entry_kind(entry)?;
            match entry.source_entity_id {
                Some(0) => {
                    kind.is_none().then_some(())?;
                    source_zero_count += 1;
                }
                Some(source_id) => {
                    run.insert(source_id, kind).is_none().then_some(())?;
                }
                None => kind.is_none().then_some(())?,
            }
            index += 1;
        }
        runs.push(run);
    }
    (source_zero_count <= 1
        && framed_class_200_count
            == table
                .entries
                .iter()
                .filter(|entry| entry.class_id == 200)
                .count())
    .then_some(())?;
    let materialized = runs
        .iter()
        .filter(|run| run.values().any(Option::is_some))
        .collect::<Vec<_>>();
    let [first, second] = materialized.as_slice() else {
        return None;
    };
    (first.keys().eq(second.keys())).then_some(())?;
    let mut paired_by_source = BTreeMap::new();
    for (source_id, first_kind) in *first {
        paired_by_source.insert(*source_id, [*first_kind, *second.get(source_id)?]);
    }
    Some(paired_by_source)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SimpleDrilledDimensionFamily {
    ExternalId2Depth,
    ExternalId4Depth,
}

impl SimpleDrilledDimensionFamily {
    pub(super) fn depth_external_id(self) -> u32 {
        match self {
            Self::ExternalId2Depth => 2,
            Self::ExternalId4Depth => 4,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SimpleDrilledHoleRecipe<'a> {
    pub(super) table: &'a crate::feature::FeatureEntityTable,
    pub(super) dimension_family: SimpleDrilledDimensionFamily,
}

pub(super) fn simple_drilled_hole_recipe<'a>(
    feature_id: u32,
    tables: &'a [crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<SimpleDrilledHoleRecipe<'a>> {
    let candidates = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .filter_map(|table| {
            let generated_by_source =
                paired_hole_replay_surfaces_by_source(feature_id, table, rows)?;
            let recipe_groups = generated_by_source.values().collect::<Vec<_>>();
            let paired = |kind| {
                recipe_groups
                    .iter()
                    .filter(|entries| entries.as_slice() == [Some(kind), Some(kind)])
                    .count()
            };
            let rowless = recipe_groups
                .iter()
                .filter(|entries| entries.as_slice() == [None, None])
                .count();
            let dimension_family = match rowless {
                2 => SimpleDrilledDimensionFamily::ExternalId2Depth,
                3 => SimpleDrilledDimensionFamily::ExternalId4Depth,
                _ => return None,
            };
            (paired(crate::surface::SurfaceKind::Cone) == 1
                && paired(crate::surface::SurfaceKind::Cylinder) == 1
                && recipe_groups.len() == rowless + 2)
                .then_some(SimpleDrilledHoleRecipe {
                    table,
                    dimension_family,
                })
        })
        .collect::<Vec<_>>();
    let [recipe] = candidates.as_slice() else {
        return None;
    };
    Some(*recipe)
}

pub(super) fn simple_drilled_hole_envelope_spans(
    scan: &ContainerScan,
    table: &crate::feature::FeatureEntityTable,
) -> Option<[[Option<f64>; 2]; 3]> {
    let [first, second] = simple_drilled_hole_corner_envelopes(scan, table)?;
    paired_corner_envelope_axis_spans(first, second)
}

pub(super) fn simple_drilled_hole_corner_envelopes(
    scan: &ContainerScan,
    table: &crate::feature::FeatureEntityTable,
) -> Option<[[[f64; 3]; 2]; 2]> {
    let feature_id = table.feature_id?;
    let envelopes = table
        .surface_ids
        .iter()
        .filter_map(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id)
                .filter(|row| row.feature_id == feature_id)
                .filter(|row| row.kind == crate::surface::SurfaceKind::Cylinder)
        })
        .map(|row| {
            unique_surface_parameter_record(scan, row)?
                .type24_terminal_corner_envelope(row.type_byte)
        })
        .collect::<Option<Vec<_>>>()?;
    let [first, second] = envelopes.as_slice() else {
        return None;
    };
    Some([*first, *second])
}

pub(super) fn simple_drilled_hole_cone_terminal_points(
    scan: &ContainerScan,
    table: &crate::feature::FeatureEntityTable,
) -> Option<[[f64; 3]; 2]> {
    let feature_id = table.feature_id?;
    let points = table
        .surface_ids
        .iter()
        .filter_map(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id)
                .filter(|row| row.feature_id == feature_id)
                .filter(|row| row.kind == crate::surface::SurfaceKind::Cone)
        })
        .map(|row| {
            let record = unique_surface_parameter_record(scan, row)?;
            (record.boundary == crate::surface::SurfaceBodyBoundary::CompoundClose
                && record.scalar_tokens.len() == 7)
                .then_some(())?;
            record.scalar_tokens[4..]
                .iter()
                .map(|slot| slot.value.filter(|value| value.is_finite()))
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()
        })
        .collect::<Option<Vec<_>>>()?;
    points.try_into().ok()
}

pub(super) fn simple_drilled_hole_placement(
    scan: &ContainerScan,
    table: &crate::feature::FeatureEntityTable,
    diameter: f64,
    depth: f64,
) -> Option<(Point3, Vector3)> {
    let corners = simple_drilled_hole_corner_envelopes(scan, table)?;
    drilled_hole_placement_from_corner_envelopes(corners, diameter, depth).or_else(|| {
        clipped_drilled_hole_placement_from_cone_points(
            corners,
            simple_drilled_hole_cone_terminal_points(scan, table)?,
            diameter,
            depth,
        )
    })
}

pub(super) fn simple_drilled_hole_axis_placement(
    scan: &ContainerScan,
    table: &crate::feature::FeatureEntityTable,
    diameter: f64,
) -> Option<cadmpeg_ir::features::HolePlacement> {
    let feature_id = table.feature_id?;
    let cylinder_ids = table
        .surface_ids
        .iter()
        .copied()
        .filter(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_some_and(
                |row| {
                    row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                },
            )
        })
        .collect::<BTreeSet<_>>();
    let frames =
        unique_available_positional_cylinder_frames(&cylinder_ids, &scan.surfaces.parameters)?;
    simple_drilled_axis_placement_from_frames(&frames, diameter)
}

pub(super) fn simple_drilled_axis_placement_from_frames(
    frames: &[crate::surface::PositionalCylinderFrame],
    diameter: f64,
) -> Option<cadmpeg_ir::features::HolePlacement> {
    let first = *frames.first()?;
    let axis = normalized(first.axis)?;
    let coordinate_scale = frames
        .iter()
        .flat_map(|frame| frame.origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    (diameter.is_finite() && diameter > 0.0 && first.origin.into_iter().all(f64::is_finite))
        .then_some(())?;
    let radius = 0.5 * diameter;
    frames
        .iter()
        .all(|frame| {
            let Some(candidate_axis) = normalized(frame.axis) else {
                return false;
            };
            let radius_scale = frame.radius.abs().max(radius.abs()).max(1.0);
            if !frame.origin.into_iter().all(f64::is_finite)
                || !frame.radius.is_finite()
                || frame.radius <= 0.0
                || (frame.radius - radius).abs() > 1e-9 * radius_scale
            {
                return false;
            }
            let alignment = axis
                .into_iter()
                .zip(candidate_axis)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if alignment.abs() < 1.0 - 1e-9 {
                return false;
            }
            let delta =
                std::array::from_fn::<_, 3, _>(|index| frame.origin[index] - first.origin[index]);
            let axial_delta = delta
                .into_iter()
                .zip(axis)
                .map(|(component, axis)| component * axis)
                .sum::<f64>();
            delta
                .into_iter()
                .zip(axis)
                .map(|(component, axis)| component - axial_delta * axis)
                .map(|component| component * component)
                .sum::<f64>()
                .sqrt()
                <= 1e-9 * coordinate_scale
        })
        .then_some(cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(first.origin[0], first.origin[1], first.origin[2]),
            axis: Vector3::new(axis[0], axis[1], axis[2]),
        })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DrilledHoleEnvelopeLayout {
    pub(super) corners: [[[f64; 3]; 2]; 2],
    pub(super) intervals: [[[f64; 2]; 3]; 2],
    pub(super) axis: usize,
    pub(super) radial: [usize; 2],
    pub(super) axial_delta: f64,
    pub(super) scale: f64,
}

pub(super) fn drilled_hole_envelope_layout(
    corners: [[[f64; 3]; 2]; 2],
    diameter: f64,
    depth: f64,
) -> Option<DrilledHoleEnvelopeLayout> {
    corners
        .iter()
        .flatten()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(())?;
    let scale = corners
        .iter()
        .flatten()
        .flatten()
        .chain([&diameter, &depth])
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (diameter > 1e-12 * scale && depth > 1e-12 * scale).then_some(())?;
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let intervals = corners.map(|patch| {
        std::array::from_fn::<_, 3, _>(|axis| {
            [
                patch[0][axis].min(patch[1][axis]),
                patch[0][axis].max(patch[1][axis]),
            ]
        })
    });
    let shared = |axis: usize| {
        close(intervals[0][axis][0], intervals[1][axis][0])
            && close(intervals[0][axis][1], intervals[1][axis][1])
    };
    let span = |axis: usize| {
        intervals[0][axis][1].max(intervals[1][axis][1])
            - intervals[0][axis][0].min(intervals[1][axis][0])
    };
    let axial_axes = (0..3)
        .filter(|axis| shared(*axis) && close(span(*axis), depth))
        .collect::<Vec<_>>();
    let [axis] = axial_axes.as_slice() else {
        return None;
    };
    let radial = (0..3)
        .filter(|candidate| candidate != axis)
        .collect::<Vec<_>>();
    let [first_radial, second_radial] = radial.as_slice() else {
        unreachable!("three-dimensional axis complement")
    };
    let axial_deltas = corners.map(|patch| patch[1][*axis] - patch[0][*axis]);
    (close(axial_deltas[0], axial_deltas[1]) && close(axial_deltas[0].abs(), depth))
        .then_some(())?;
    Some(DrilledHoleEnvelopeLayout {
        corners,
        intervals,
        axis: *axis,
        radial: [*first_radial, *second_radial],
        axial_delta: axial_deltas[0],
        scale,
    })
}

pub(super) fn drilled_hole_layout_close(
    layout: &DrilledHoleEnvelopeLayout,
    left: f64,
    right: f64,
) -> bool {
    (left - right).abs() <= 1e-9 * layout.scale
}

pub(super) fn drilled_hole_layout_shared(layout: &DrilledHoleEnvelopeLayout, axis: usize) -> bool {
    drilled_hole_layout_close(
        layout,
        layout.intervals[0][axis][0],
        layout.intervals[1][axis][0],
    ) && drilled_hole_layout_close(
        layout,
        layout.intervals[0][axis][1],
        layout.intervals[1][axis][1],
    )
}

pub(super) fn drilled_hole_layout_adjacent(
    layout: &DrilledHoleEnvelopeLayout,
    axis: usize,
) -> bool {
    drilled_hole_layout_close(
        layout,
        layout.intervals[0][axis][1],
        layout.intervals[1][axis][0],
    ) || drilled_hole_layout_close(
        layout,
        layout.intervals[1][axis][1],
        layout.intervals[0][axis][0],
    )
}

pub(super) fn drilled_hole_layout_span(layout: &DrilledHoleEnvelopeLayout, axis: usize) -> f64 {
    layout.intervals[0][axis][1].max(layout.intervals[1][axis][1])
        - layout.intervals[0][axis][0].min(layout.intervals[1][axis][0])
}

pub(super) fn drilled_hole_layout_placement(
    layout: &DrilledHoleEnvelopeLayout,
    radial_coordinates: [f64; 2],
) -> (Point3, Vector3) {
    let mut position = [0.0; 3];
    position[layout.axis] = f64::midpoint(
        layout.corners[0][0][layout.axis],
        layout.corners[1][0][layout.axis],
    );
    for (radial_axis, coordinate) in layout.radial.into_iter().zip(radial_coordinates) {
        position[radial_axis] = coordinate;
    }
    let mut direction = [0.0; 3];
    direction[layout.axis] = layout.axial_delta.signum();
    (
        Point3::new(position[0], position[1], position[2]),
        Vector3::new(direction[0], direction[1], direction[2]),
    )
}

pub(super) fn drilled_hole_placement_from_corner_envelopes(
    corners: [[[f64; 3]; 2]; 2],
    diameter: f64,
    depth: f64,
) -> Option<(Point3, Vector3)> {
    let layout = drilled_hole_envelope_layout(corners, diameter, depth)?;
    let radial_forms = layout.radial.map(|radial_axis| {
        (
            drilled_hole_layout_shared(&layout, radial_axis),
            drilled_hole_layout_adjacent(&layout, radial_axis),
            drilled_hole_layout_span(&layout, radial_axis),
        )
    });
    let complementary =
        (radial_forms[0].0 && radial_forms[1].1) || (radial_forms[0].1 && radial_forms[1].0);
    if complementary
        && radial_forms
            .iter()
            .all(|(_, _, span)| drilled_hole_layout_close(&layout, *span, diameter))
    {
        let radial_coordinates = layout.radial.map(|radial_axis| {
            f64::midpoint(
                layout.intervals[0][radial_axis][0].min(layout.intervals[1][radial_axis][0]),
                layout.intervals[0][radial_axis][1].max(layout.intervals[1][radial_axis][1]),
            )
        });
        return Some(drilled_hole_layout_placement(&layout, radial_coordinates));
    }

    let nonshared_bounds = layout.radial.map(|radial_axis| {
        let intervals = layout.intervals.map(|patch| patch[radial_axis]);
        match (
            drilled_hole_layout_close(&layout, intervals[0][0], intervals[1][0]),
            drilled_hole_layout_close(&layout, intervals[0][1], intervals[1][1]),
        ) {
            (true, false) => Some([intervals[0][1], intervals[1][1]]),
            (false, true) => Some([intervals[0][0], intervals[1][0]]),
            _ => None,
        }
    });
    let common_diameter = layout.radial.map(|radial_axis| {
        drilled_hole_layout_shared(&layout, radial_axis)
            && drilled_hole_layout_close(
                &layout,
                drilled_hole_layout_span(&layout, radial_axis),
                diameter,
            )
    });
    let (clipped_index, [first, second]) = match (common_diameter, nonshared_bounds) {
        ([true, false], [None, Some(bounds)]) => (1, bounds),
        ([false, true], [Some(bounds), None]) => (0, bounds),
        _ => return None,
    };
    drilled_hole_layout_close(&layout, (first - second).abs(), diameter).then_some(())?;
    let radial_coordinates = std::array::from_fn(|index| {
        let radial_axis = layout.radial[index];
        if index == clipped_index {
            f64::midpoint(first, second)
        } else {
            f64::midpoint(
                layout.intervals[0][radial_axis][0],
                layout.intervals[0][radial_axis][1],
            )
        }
    });
    Some(drilled_hole_layout_placement(&layout, radial_coordinates))
}

pub(super) fn clipped_drilled_hole_placement_from_cone_points(
    corners: [[[f64; 3]; 2]; 2],
    cone_points: [[f64; 3]; 2],
    diameter: f64,
    depth: f64,
) -> Option<(Point3, Vector3)> {
    let layout = drilled_hole_envelope_layout(corners, diameter, depth)?;
    cone_points
        .iter()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(())?;
    let adjacent_diameter = layout
        .radial
        .iter()
        .copied()
        .filter(|axis| {
            drilled_hole_layout_adjacent(&layout, *axis)
                && drilled_hole_layout_close(
                    &layout,
                    drilled_hole_layout_span(&layout, *axis),
                    diameter,
                )
        })
        .collect::<Vec<_>>();
    let [diameter_axis] = adjacent_diameter.as_slice() else {
        return None;
    };
    let clipped_axis = layout
        .radial
        .iter()
        .copied()
        .find(|axis| axis != diameter_axis)?;
    (drilled_hole_layout_shared(&layout, clipped_axis)
        && drilled_hole_layout_span(&layout, clipped_axis) > 0.0
        && !drilled_hole_layout_close(
            &layout,
            drilled_hole_layout_span(&layout, clipped_axis),
            diameter,
        ))
    .then_some(())?;
    (0..2)
        .all(|patch| {
            drilled_hole_layout_close(
                &layout,
                cone_points[patch][layout.axis],
                corners[patch][0][layout.axis],
            ) && layout.radial.iter().all(|axis| {
                drilled_hole_layout_close(
                    &layout,
                    cone_points[patch][*axis],
                    corners[patch][1][*axis],
                )
            })
        })
        .then_some(())?;
    drilled_hole_layout_close(
        &layout,
        cone_points[0][clipped_axis],
        cone_points[1][clipped_axis],
    )
    .then_some(())?;
    let radial_coordinates = layout.radial.map(|axis| {
        if axis == clipped_axis {
            f64::midpoint(cone_points[0][axis], cone_points[1][axis])
        } else {
            f64::midpoint(
                layout.intervals[0][axis][0].min(layout.intervals[1][axis][0]),
                layout.intervals[0][axis][1].max(layout.intervals[1][axis][1]),
            )
        }
    });
    Some(drilled_hole_layout_placement(&layout, radial_coordinates))
}

pub(super) fn paired_corner_envelope_axis_spans(
    first: [[f64; 3]; 2],
    second: [[f64; 3]; 2],
) -> Option<[[Option<f64>; 2]; 3]> {
    first
        .iter()
        .chain(&second)
        .flatten()
        .all(|value| value.is_finite())
        .then_some(())?;
    let intervals = |corners: [[f64; 3]; 2]| {
        std::array::from_fn::<_, 3, _>(|axis| {
            let values = [corners[0][axis], corners[1][axis]];
            [values[0].min(values[1]), values[0].max(values[1])]
        })
    };
    let first = intervals(first);
    let second = intervals(second);
    let spans = std::array::from_fn::<_, 3, _>(|axis| {
        let common_lower = approximately_equal(first[axis][0], second[axis][0]);
        let common_upper = approximately_equal(first[axis][1], second[axis][1]);
        let shared = common_lower && common_upper;
        let shared_span = shared.then(|| {
            f64::midpoint(
                first[axis][1] - first[axis][0],
                second[axis][1] - second[axis][0],
            )
        });
        let shared_span = shared_span.filter(|span| *span > 0.0);
        let adjacent = approximately_equal(first[axis][1], second[axis][0])
            || approximately_equal(second[axis][1], first[axis][0]);
        let adjacent_span = adjacent
            .then(|| first[axis][1].max(second[axis][1]) - first[axis][0].min(second[axis][0]));
        let one_sided_span = (common_lower != common_upper).then(|| {
            if common_lower {
                (first[axis][1] - second[axis][1]).abs()
            } else {
                (first[axis][0] - second[axis][0]).abs()
            }
        });
        let paired_span = adjacent_span.or(one_sided_span).filter(|span| *span > 0.0);
        [shared_span, paired_span]
    });
    Some(spans)
}

pub(super) fn simple_drilled_hole_dimensions(
    scan: &ContainerScan,
    observed_envelope_spans: Option<[[Option<f64>; 2]; 3]>,
    family: SimpleDrilledDimensionFamily,
) -> Option<(f64, f64, f64)> {
    simple_drilled_hole_dimension_values(
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.id == 911)
            .filter_map(|definition| definition.dimensions.as_ref()),
        observed_envelope_spans,
        family,
    )
}

pub(super) fn simple_drilled_hole_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::FeatureDimensionTable>,
    observed_envelope_spans: Option<[[Option<f64>; 2]; 3]>,
    family: SimpleDrilledDimensionFamily,
) -> Option<(f64, f64, f64)> {
    let tables = tables
        .filter(|table| feature_dimension_table_complete(table) && table.rows.len() == 3)
        .collect::<Vec<_>>();
    let depth_external_id = family.depth_external_id();
    let has_simple_drilled_signature = |table: &crate::feature::FeatureDimensionTable| {
        [
            (0, 2, crate::feature::DimensionUnit::Millimeters),
            (1, 10, crate::feature::DimensionUnit::Radians),
            (
                depth_external_id,
                2,
                crate::feature::DimensionUnit::Millimeters,
            ),
        ]
        .into_iter()
        .all(|(external_id, dimension_type, unit)| {
            table
                .rows
                .iter()
                .filter(|row| {
                    row.external_id == external_id
                        && row.dimension_type == dimension_type
                        && row.value_unit == unit
                })
                .count()
                == 1
        })
    };
    let candidates =
        tables
            .into_iter()
            .filter(|table| has_simple_drilled_signature(table))
            .map(|table| {
                let value = |external_id, dimension_type, unit| {
                    let rows = table
                        .rows
                        .iter()
                        .filter(|row| {
                            row.external_id == external_id
                                && row.dimension_type == dimension_type
                                && row.value_unit == unit
                        })
                        .collect::<Vec<_>>();
                    let [row] = rows.as_slice() else {
                        return None;
                    };
                    row.value.filter(|value| value.is_finite())
                };
                let bore_radius = value(0, 2, crate::feature::DimensionUnit::Millimeters)?;
                let signed_depth = value(
                    depth_external_id,
                    2,
                    crate::feature::DimensionUnit::Millimeters,
                )?;
                let bore_diameter = 2.0 * bore_radius;
                (bore_diameter.is_finite() && bore_diameter > 0.0 && signed_depth != 0.0)
                    .then_some(())?;
                let blind_depth = signed_depth.abs();
                if observed_envelope_spans.is_some_and(|spans| {
                    !dimension_pair_matches_envelope_spans(bore_diameter, blind_depth, spans)
                }) {
                    return Some(None);
                }
                let drill_point_angle = value(1, 10, crate::feature::DimensionUnit::Radians)?;
                (drill_point_angle > 0.0 && drill_point_angle < std::f64::consts::PI)
                    .then_some(Some((bore_diameter, drill_point_angle, blind_depth)))
            })
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
    let first = *candidates.first()?;
    candidates
        .iter()
        .all(|candidate| {
            [candidate.0, candidate.1, candidate.2]
                .into_iter()
                .zip([first.0, first.1, first.2])
                .all(|(candidate, first)| approximately_equal(candidate, first))
        })
        .then_some(first)
}

pub(super) fn dimension_pair_matches_envelope_spans(
    bore_diameter: f64,
    blind_depth: f64,
    spans: [[Option<f64>; 2]; 3],
) -> bool {
    for diameter_axis in 0..3 {
        for depth_axis in 0..3 {
            if diameter_axis != depth_axis
                && spans[diameter_axis]
                    .into_iter()
                    .flatten()
                    .any(|span| approximately_equal(span, bore_diameter))
                && spans[depth_axis]
                    .into_iter()
                    .flatten()
                    .any(|span| approximately_equal(span, blind_depth))
            {
                return true;
            }
        }
    }
    false
}

pub(super) fn counterbore_dimensions(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<(f64, f64, f64)> {
    let table = counterbore_entity_table(scan, feature_id)?;
    let generated_cylinders = table
        .surface_ids
        .iter()
        .copied()
        .filter(|surface_id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, *surface_id).is_some_and(
                |row| {
                    row.feature_id == feature_id
                        && row.kind == crate::surface::SurfaceKind::Cylinder
                },
            )
        })
        .collect::<BTreeSet<_>>();
    let generated_radii = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            let surface_id = surface
                .id
                .0
                .strip_prefix("creo:visibgeom:surface#")?
                .parse::<u32>()
                .ok()?;
            generated_cylinders.contains(&surface_id).then_some(())?;
            let SurfaceGeometry::Cylinder { radius, .. } = surface.geometry else {
                return None;
            };
            Some(radius)
        })
        .collect::<Vec<_>>();
    let dimension_tables = || {
        scan.features
            .definitions
            .iter()
            .filter(|definition| definition.id == 911)
            .filter_map(|definition| definition.dimensions.as_ref())
    };
    counterbore_dimension_values(dimension_tables(), &generated_radii).or_else(|| {
        let source_spans = counterbore_cylinder_sources(scan, feature_id)?
            .into_iter()
            .map(|ids| {
                let [first_id, second_id] = ids.as_slice() else {
                    return None;
                };
                let envelope = |id: &u32| {
                    let row = crate::surface::unique_surface_row(&scan.surfaces.rows, *id)?;
                    unique_surface_parameter_record(scan, row)?
                        .type24_terminal_corner_envelope(row.type_byte)
                };
                paired_corner_envelope_axis_spans(envelope(first_id)?, envelope(second_id)?)
            })
            .collect::<Vec<_>>();
        let [first_source, second_source] = source_spans.as_slice() else {
            return None;
        };
        if first_source.is_some() || second_source.is_some() {
            counterbore_envelope_dimension_values(dimension_tables(), &source_spans)
        } else {
            counterbore_unenveloped_dimension_values(dimension_tables())
        }
    })
}

pub(super) fn counterbore_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::FeatureDimensionTable>,
    generated_radii: &[f64],
) -> Option<(f64, f64, f64)> {
    let mut candidates = Vec::new();
    for table in tables {
        if usize::try_from(table.declared_count).ok() != Some(table.rows.len())
            || table.rows.len() != 4
        {
            continue;
        }
        let value = |external_id, dimension_type| {
            let rows = table
                .rows
                .iter()
                .filter(|row| {
                    row.external_id == external_id && row.dimension_type == dimension_type
                })
                .collect::<Vec<_>>();
            let [row] = rows.as_slice() else {
                return None;
            };
            row.value.filter(|value| value.is_finite() && *value > 0.0)
        };
        let (Some(bore_radius), Some(placement_distance), Some(depth), Some(counterbore_radius)) =
            (value(0, 2), value(1, 2), value(2, 1), value(3, 2))
        else {
            continue;
        };
        if bore_radius >= counterbore_radius
            || placement_distance <= 0.0
            || !generated_radii.iter().any(|radius| {
                (*radius - counterbore_radius).abs()
                    <= 1e-9 * radius.abs().max(counterbore_radius.abs()).max(1.0)
            })
        {
            continue;
        }
        candidates.push((2.0 * bore_radius, 2.0 * counterbore_radius, depth));
    }
    let first = *candidates.first()?;
    candidates
        .iter()
        .all(|candidate| {
            [
                candidate.0 - first.0,
                candidate.1 - first.1,
                candidate.2 - first.2,
            ]
            .iter()
            .all(|delta| delta.abs() <= 1e-9)
        })
        .then_some(first)
}

pub(super) fn counterbore_envelope_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::FeatureDimensionTable>,
    source_spans: &[Option<[[Option<f64>; 2]; 3]>],
) -> Option<(f64, f64, f64)> {
    let [first_source, second_source] = source_spans else {
        return None;
    };
    let cylinder_diameter_matches = |diameter: f64, spans: [[Option<f64>; 2]; 3]| {
        (0..3)
            .filter(|axis| {
                spans[*axis]
                    .into_iter()
                    .flatten()
                    .any(|span| approximately_equal(span, diameter))
            })
            .count()
            == 2
    };
    let counterbore_matches = |diameter: f64, depth: f64, spans: [[Option<f64>; 2]; 3]| {
        let diameter_axes = (0..3)
            .filter(|axis| {
                spans[*axis]
                    .into_iter()
                    .flatten()
                    .any(|span| approximately_equal(span, diameter))
            })
            .collect::<Vec<_>>();
        let [first_axis, second_axis] = diameter_axes.as_slice() else {
            return false;
        };
        (0..3)
            .find(|axis| axis != first_axis && axis != second_axis)
            .is_some_and(|axis| {
                spans[axis]
                    .into_iter()
                    .flatten()
                    .any(|span| approximately_equal(span, depth))
            })
    };
    let candidates = tables
        .filter_map(|table| {
            let (bore_diameter, counterbore_diameter, counterbore_depth) =
                counterbore_envelope_dimension_tuple(table)?;
            let matches = match (first_source, second_source) {
                (Some(first), Some(second)) => {
                    [
                        cylinder_diameter_matches(bore_diameter, *first)
                            && counterbore_matches(
                                counterbore_diameter,
                                counterbore_depth,
                                *second,
                            ),
                        cylinder_diameter_matches(bore_diameter, *second)
                            && counterbore_matches(counterbore_diameter, counterbore_depth, *first),
                    ]
                    .into_iter()
                    .filter(|matches| *matches)
                    .count()
                        == 1
                }
                (Some(spans), None) | (None, Some(spans)) => {
                    cylinder_diameter_matches(bore_diameter, *spans)
                        != counterbore_matches(counterbore_diameter, counterbore_depth, *spans)
                }
                (None, None) => false,
            };
            matches.then_some((bore_diameter, counterbore_diameter, counterbore_depth))
        })
        .collect::<Vec<_>>();
    unique_counterbore_dimension_tuple(&candidates)
}

pub(super) fn counterbore_unenveloped_dimension_values<'a>(
    tables: impl Iterator<Item = &'a crate::feature::FeatureDimensionTable>,
) -> Option<(f64, f64, f64)> {
    let candidates = tables
        .filter(|table| {
            feature_dimension_table_complete(table) && matches!(table.rows.len(), 4 | 5)
        })
        .map(counterbore_envelope_dimension_tuple)
        .collect::<Option<Vec<_>>>()?;
    unique_counterbore_dimension_tuple(&candidates)
}

pub(super) fn counterbore_envelope_dimension_tuple(
    table: &crate::feature::FeatureDimensionTable,
) -> Option<(f64, f64, f64)> {
    (feature_dimension_table_complete(table) && matches!(table.rows.len(), 4 | 5)).then_some(())?;
    let value = |external_id, dimension_type, unit| {
        let rows = table
            .rows
            .iter()
            .filter(|row| {
                row.external_id == external_id
                    && row.dimension_type == dimension_type
                    && row.value_unit == unit
            })
            .collect::<Vec<_>>();
        let [row] = rows.as_slice() else {
            return None;
        };
        row.value.filter(|value| value.is_finite())
    };
    let signed_counterbore_depth = value(0, 1, crate::feature::DimensionUnit::Millimeters)?;
    let bore_radius = value(1, 2, crate::feature::DimensionUnit::Millimeters)?;
    let (counterbore_radius, _placement_distance) = if table.rows.len() == 4 {
        let shifted = value(2, 2, crate::feature::DimensionUnit::Millimeters).zip(value(
            3,
            2,
            crate::feature::DimensionUnit::Millimeters,
        ));
        let retained = value(3, 2, crate::feature::DimensionUnit::Millimeters).zip(value(
            4,
            2,
            crate::feature::DimensionUnit::Millimeters,
        ));
        match (shifted, retained) {
            (Some(layout), None) | (None, Some(layout)) => layout,
            _ => return None,
        }
    } else {
        let drill_point_angle = value(2, 10, crate::feature::DimensionUnit::Radians)?;
        (drill_point_angle > 0.0 && drill_point_angle < std::f64::consts::PI).then_some(())?;
        (
            value(3, 2, crate::feature::DimensionUnit::Millimeters)?,
            value(4, 2, crate::feature::DimensionUnit::Millimeters)?,
        )
    };
    (signed_counterbore_depth != 0.0 && bore_radius > 0.0 && counterbore_radius > bore_radius)
        .then_some(())?;
    Some((
        2.0 * bore_radius,
        2.0 * counterbore_radius,
        signed_counterbore_depth.abs(),
    ))
}

pub(super) fn unique_counterbore_dimension_tuple(
    candidates: &[(f64, f64, f64)],
) -> Option<(f64, f64, f64)> {
    let first = *candidates.first()?;
    candidates
        .iter()
        .all(|candidate| {
            [candidate.0, candidate.1, candidate.2]
                .into_iter()
                .zip([first.0, first.1, first.2])
                .all(|(candidate, first)| approximately_equal(candidate, first))
        })
        .then_some(first)
}

pub(super) fn counterbore_patch_geometries(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<Vec<(u32, SurfaceGeometry)>> {
    let (bore_diameter, counterbore_diameter, _) = counterbore_dimensions(scan, ir, feature_id)?;
    let cylinder_sources = counterbore_cylinder_sources(scan, feature_id)?;
    let existing_geometries = ir
        .model
        .surfaces
        .iter()
        .filter_map(|surface| {
            let id = surface
                .id
                .0
                .strip_prefix("creo:visibgeom:surface#")?
                .parse::<u32>()
                .ok()?;
            Some((id, surface.geometry.clone()))
        })
        .collect::<BTreeMap<_, _>>();
    counterbore_source_patch_geometries(
        &cylinder_sources,
        &existing_geometries,
        bore_diameter,
        counterbore_diameter,
    )
}

pub(super) fn counterbore_cylinder_sources(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<Vec<Vec<u32>>> {
    let table = counterbore_entity_table(scan, feature_id)?;
    let mut cylinders_by_source = BTreeMap::<u32, Vec<u32>>::new();
    for entry in table.entries.iter().filter(|entry| entry.class_id == 200) {
        if !table.surface_ids.contains(&entry.entity_id) {
            continue;
        }
        let source_id = entry.source_entity_id?;
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
        else {
            continue;
        };
        if row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder {
            cylinders_by_source
                .entry(source_id)
                .or_default()
                .push(entry.entity_id);
        }
    }
    Some(
        cylinders_by_source
            .values()
            .filter(|ids| ids.len() == 2)
            .cloned()
            .collect(),
    )
}

pub(super) fn counterbore_entity_table<'a>(
    scan: &'a ContainerScan<'_>,
    feature_id: u32,
) -> Option<&'a crate::feature::FeatureEntityTable> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .filter(|table| {
            table.entries.iter().any(|entry| {
                entry.class_id == 200
                    && entry.source_entity_id.is_some()
                    && table.surface_ids.contains(&entry.entity_id)
                    && crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
                        .is_some_and(|row| {
                            row.feature_id == feature_id
                                && row.kind == crate::surface::SurfaceKind::Cylinder
                        })
            })
        })
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    Some(*table)
}

pub(super) fn counterbore_axis_placement(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<cadmpeg_ir::features::HolePlacement> {
    let cylinder_axis =
        counterbore_dimensions(scan, ir, feature_id).and_then(|(_, counterbore_diameter, _)| {
            let cylinder_sources = counterbore_cylinder_sources(scan, feature_id)?;
            let existing_geometries = ir
                .model
                .surfaces
                .iter()
                .filter_map(|surface| {
                    let id = surface
                        .id
                        .0
                        .strip_prefix("creo:visibgeom:surface#")?
                        .parse::<u32>()
                        .ok()?;
                    Some((id, surface.geometry.clone()))
                })
                .collect::<BTreeMap<_, _>>();
            counterbore_axis_placement_from_sources(
                &cylinder_sources,
                &existing_geometries,
                counterbore_diameter,
            )
        });
    cylinder_axis.or_else(|| {
        counterbore_support_axis_placement(
            feature_id,
            counterbore_entity_table(scan, feature_id)?,
            &scan.surfaces.rows,
            &scan.planes.local_systems,
        )
    })
}

pub(super) fn counterbore_support_axis_placement(
    feature_id: u32,
    table: &crate::feature::FeatureEntityTable,
    rows: &[crate::surface::SurfaceRow],
    frames: &[crate::surface::PlaneLocalSystem],
) -> Option<cadmpeg_ir::features::HolePlacement> {
    (table.feature_id == Some(feature_id)).then_some(())?;
    let plane_ids = table
        .surface_ids
        .iter()
        .copied()
        .filter(|surface_id| {
            crate::surface::unique_surface_row(rows, *surface_id).is_some_and(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
            })
        })
        .collect::<Vec<_>>();
    let [plane_id] = plane_ids.as_slice() else {
        return None;
    };
    let matching_frames = frames
        .iter()
        .filter(|frame| frame.surface_id == *plane_id)
        .collect::<Vec<_>>();
    let [frame] = matching_frames.as_slice() else {
        return None;
    };
    let origin = frame
        .origin
        .filter(|origin| origin.iter().all(|value| value.is_finite()))?;
    let axis = normalized(frame.normal?)?;
    Some(cadmpeg_ir::features::HolePlacement::Axis {
        origin: Point3::new(origin[0], origin[1], origin[2]),
        axis: Vector3::new(axis[0], axis[1], axis[2]),
    })
}

pub(super) fn counterbore_axis_placement_from_sources(
    cylinder_sources: &[Vec<u32>],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    counterbore_diameter: f64,
) -> Option<cadmpeg_ir::features::HolePlacement> {
    let carriers = cylinder_sources
        .iter()
        .filter_map(|ids| {
            complete_cylinder_source_carrier(ids, existing_geometries, 0.5 * counterbore_diameter)
        })
        .collect::<Vec<_>>();
    let [carrier] = carriers.as_slice() else {
        return None;
    };
    let SurfaceGeometry::Cylinder { origin, axis, .. } = carrier else {
        unreachable!("cylinder carrier helper returns a cylinder")
    };
    Some(cadmpeg_ir::features::HolePlacement::Axis {
        origin: *origin,
        axis: *axis,
    })
}

pub(super) fn counterbore_directed_placement(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<(Option<u32>, Point3, Vector3, Termination)> {
    let (bore_diameter, counterbore_diameter, counterbore_depth) =
        counterbore_dimensions(scan, ir, feature_id)?;
    let sources = counterbore_cylinder_sources(scan, feature_id)?;
    let [first, second] = sources.as_slice() else {
        return None;
    };
    let boundary = |ids: &[u32], radius: f64| {
        counterbore_source_boundary_circle(scan, ir, feature_id, ids, radius)
    };
    let bore_radius = 0.5 * bore_diameter;
    let counterbore_radius = 0.5 * counterbore_diameter;
    let boundaries = (
        boundary(first, counterbore_radius),
        boundary(first, bore_radius),
        boundary(second, counterbore_radius),
        boundary(second, bore_radius),
    );
    let boundary_placement = match boundaries {
        (Some(counterbore), None, None, Some(bore))
        | (None, Some(bore), Some(counterbore), None) => {
            counterbore_directed_span(counterbore, bore, counterbore_depth).map(
                |(face, position, direction, extent)| (Some(face), position, direction, extent),
            )
        }
        _ => None,
    };
    boundary_placement.or_else(|| {
        let source_corners = sources
            .iter()
            .map(|ids| {
                let [first_id, second_id] = ids.as_slice() else {
                    return None;
                };
                let envelope = |id| {
                    let row = crate::surface::unique_surface_row(&scan.surfaces.rows, id)?;
                    unique_surface_parameter_record(scan, row)?
                        .type24_terminal_corner_envelope(row.type_byte)
                };
                Some([envelope(*first_id)?, envelope(*second_id)?])
            })
            .collect::<Option<Vec<_>>>()?;
        counterbore_placement_from_corner_envelopes(
            &source_corners,
            bore_diameter,
            counterbore_diameter,
            counterbore_depth,
        )
        .map(|(position, direction, extent)| (None, position, direction, extent))
    })
}

#[derive(Debug, Clone, Copy)]
pub(super) struct CounterboreEnvelopeLayout {
    pub(super) axis: usize,
    pub(super) radial: [usize; 2],
    pub(super) center: [f64; 3],
    pub(super) axial_interval: [f64; 2],
}

pub(super) fn counterbore_source_envelope_layout(
    corners: [[[f64; 3]; 2]; 2],
    diameter: f64,
    axial_depth: Option<f64>,
    scale: f64,
) -> Option<CounterboreEnvelopeLayout> {
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let intervals = corners.map(|patch| {
        std::array::from_fn::<_, 3, _>(|axis| {
            [
                patch[0][axis].min(patch[1][axis]),
                patch[0][axis].max(patch[1][axis]),
            ]
        })
    });
    let shared = |axis: usize| {
        close(intervals[0][axis][0], intervals[1][axis][0])
            && close(intervals[0][axis][1], intervals[1][axis][1])
    };
    let adjacent = |axis: usize| {
        close(intervals[0][axis][1], intervals[1][axis][0])
            || close(intervals[1][axis][1], intervals[0][axis][0])
    };
    let union = |axis: usize| {
        [
            intervals[0][axis][0].min(intervals[1][axis][0]),
            intervals[0][axis][1].max(intervals[1][axis][1]),
        ]
    };
    let diameter_axes = (0..3)
        .filter(|axis| {
            let union = union(*axis);
            (shared(*axis) || adjacent(*axis)) && close(union[1] - union[0], diameter)
        })
        .collect::<Vec<_>>();
    let [first_radial, second_radial] = diameter_axes.as_slice() else {
        return None;
    };
    let axis = (0..3).find(|axis| axis != first_radial && axis != second_radial)?;
    shared(axis).then_some(())?;
    let axial_interval = intervals[0][axis];
    let axial_span = axial_interval[1] - axial_interval[0];
    (axial_span > 0.0 && axial_depth.is_none_or(|depth| close(axial_span, depth))).then_some(())?;
    let mut center = [0.0; 3];
    for radial_axis in [*first_radial, *second_radial] {
        let bounds = union(radial_axis);
        center[radial_axis] = f64::midpoint(bounds[0], bounds[1]);
    }
    Some(CounterboreEnvelopeLayout {
        axis,
        radial: [*first_radial, *second_radial],
        center,
        axial_interval,
    })
}

pub(super) fn counterbore_placement_from_corner_envelopes(
    source_corners: &[[[[f64; 3]; 2]; 2]],
    bore_diameter: f64,
    counterbore_diameter: f64,
    counterbore_depth: f64,
) -> Option<(Point3, Vector3, Termination)> {
    let [first_source, second_source] = source_corners else {
        return None;
    };
    let scale = source_corners
        .iter()
        .flatten()
        .flatten()
        .flatten()
        .chain([&bore_diameter, &counterbore_diameter, &counterbore_depth])
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    source_corners
        .iter()
        .flatten()
        .flatten()
        .flatten()
        .all(|value| value.is_finite())
        .then_some(())?;
    (bore_diameter > 0.0 && counterbore_diameter > bore_diameter && counterbore_depth > 0.0)
        .then_some(())?;
    let close = |left: f64, right: f64| (left - right).abs() <= 1e-9 * scale;
    let assignments = [
        (
            counterbore_source_envelope_layout(*first_source, bore_diameter, None, scale),
            counterbore_source_envelope_layout(
                *second_source,
                counterbore_diameter,
                Some(counterbore_depth),
                scale,
            ),
        ),
        (
            counterbore_source_envelope_layout(*second_source, bore_diameter, None, scale),
            counterbore_source_envelope_layout(
                *first_source,
                counterbore_diameter,
                Some(counterbore_depth),
                scale,
            ),
        ),
    ]
    .into_iter()
    .filter_map(|(bore, counterbore)| Some((bore?, counterbore?)))
    .collect::<Vec<_>>();
    let [(bore, counterbore)] = assignments.as_slice() else {
        return None;
    };
    (bore.axis == counterbore.axis && bore.radial == counterbore.radial).then_some(())?;
    bore.radial
        .iter()
        .all(|axis| close(bore.center[*axis], counterbore.center[*axis]))
        .then_some(())?;
    let (entry, direction_sign, length) =
        if close(counterbore.axial_interval[1], bore.axial_interval[0]) {
            (
                counterbore.axial_interval[0],
                1.0,
                bore.axial_interval[1] - counterbore.axial_interval[0],
            )
        } else if close(bore.axial_interval[1], counterbore.axial_interval[0]) {
            (
                counterbore.axial_interval[1],
                -1.0,
                counterbore.axial_interval[1] - bore.axial_interval[0],
            )
        } else {
            return None;
        };
    (length > counterbore_depth && length.is_finite()).then_some(())?;
    let mut position = counterbore.center;
    position[counterbore.axis] = entry;
    let mut direction = [0.0; 3];
    direction[counterbore.axis] = direction_sign;
    Some((
        Point3::new(position[0], position[1], position[2]),
        Vector3::new(direction[0], direction[1], direction[2]),
        Termination::Blind {
            length: Length(length),
        },
    ))
}

pub(super) fn counterbore_directed_span(
    counterbore: (u32, Point3, [f64; 3]),
    bore: (u32, Point3, [f64; 3]),
    counterbore_depth: f64,
) -> Option<(u32, Point3, Vector3, Termination)> {
    let delta = [
        bore.1.x - counterbore.1.x,
        bore.1.y - counterbore.1.y,
        bore.1.z - counterbore.1.z,
    ];
    let length = delta.iter().map(|value| value * value).sum::<f64>().sqrt();
    let scale = [
        counterbore.1.x,
        counterbore.1.y,
        counterbore.1.z,
        bore.1.x,
        bore.1.y,
        bore.1.z,
        counterbore_depth,
    ]
    .into_iter()
    .map(f64::abs)
    .fold(1.0, f64::max);
    (length.is_finite() && length > 1e-12 * scale && counterbore_depth <= length + 1e-9 * scale)
        .then_some(())?;
    let direction = delta.map(|value| value / length);
    [counterbore.2, bore.2]
        .iter()
        .all(|axis| {
            let alignment = direction
                .iter()
                .zip(axis)
                .map(|(left, right)| left * right)
                .sum::<f64>()
                .abs();
            (alignment - 1.0).abs() <= 1e-9
        })
        .then_some(())?;
    Some((
        counterbore.0,
        counterbore.1,
        Vector3::new(direction[0], direction[1], direction[2]),
        Termination::Blind {
            length: Length(length),
        },
    ))
}

pub(super) fn counterbore_source_boundary_circle(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    cylinder_ids: &[u32],
    radius: f64,
) -> Option<(u32, Point3, [f64; 3])> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .map(|row| (row.id, row))
        .collect::<BTreeMap<_, _>>();
    let boundary_for = |cylinder_id| {
        let boundaries = crate::topology::uniquely_identified_rows(&scan.curves.topology_rows)
            .into_iter()
            .filter_map(|edge| {
                (edge.feature_id == feature_id && edge.type_byte == 0).then_some(())?;
                let other = match edge.faces {
                    [left, right] if left == cylinder_id => right,
                    [left, right] if right == cylinder_id => left,
                    _ => return None,
                };
                let plane = rows.get(&other)?;
                (plane.kind == crate::surface::SurfaceKind::Plane).then_some(())?;
                let curve = ir.model.curves.iter().find(|curve| {
                    curve.id == CurveId(format!("creo:visibgeom:curve#{}", edge.id))
                })?;
                let CurveGeometry::Circle {
                    center,
                    axis,
                    radius: candidate,
                    ..
                } = &curve.geometry
                else {
                    return None;
                };
                ((*candidate - radius).abs() <= 1e-9).then_some(())?;
                let axis = normalized([axis.x, axis.y, axis.z])?;
                let surface = ir.model.surfaces.iter().find(|surface| {
                    surface.id == SurfaceId(format!("creo:visibgeom:surface#{other}"))
                })?;
                let SurfaceGeometry::Plane { origin, normal, .. } = &surface.geometry else {
                    return None;
                };
                let normal = normalized([normal.x, normal.y, normal.z])?;
                let alignment = axis
                    .iter()
                    .zip(normal)
                    .map(|(left, right)| left * right)
                    .sum::<f64>()
                    .abs();
                let distance = [
                    center.x - origin.x,
                    center.y - origin.y,
                    center.z - origin.z,
                ]
                .iter()
                .zip(normal)
                .map(|(delta, normal)| delta * normal)
                .sum::<f64>()
                .abs();
                let scale = [
                    center.x, center.y, center.z, origin.x, origin.y, origin.z, radius,
                ]
                .into_iter()
                .map(f64::abs)
                .fold(1.0, f64::max);
                ((alignment - 1.0).abs() <= 1e-9 && distance <= 1e-9 * scale).then_some(())?;
                Some((other, *center, axis))
            })
            .collect::<Vec<_>>();
        let [boundary] = boundaries.as_slice() else {
            return None;
        };
        Some(*boundary)
    };
    let boundaries = cylinder_ids
        .iter()
        .copied()
        .map(boundary_for)
        .collect::<Option<Vec<_>>>()?;
    let first = *boundaries.first()?;
    boundaries
        .iter()
        .all(|candidate| {
            candidate.0 == first.0
                && candidate.1 == first.1
                && candidate
                    .2
                    .iter()
                    .zip(first.2)
                    .map(|(left, right)| left * right)
                    .sum::<f64>()
                    .abs()
                    >= 1.0 - 1e-9
        })
        .then_some(first)
}

pub(super) fn counterbore_source_patch_geometries(
    cylinder_sources: &[Vec<u32>],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    bore_diameter: f64,
    counterbore_diameter: f64,
) -> Option<Vec<(u32, SurfaceGeometry)>> {
    let [first_source, second_source] = cylinder_sources else {
        return None;
    };
    let counterbore_radius = 0.5 * counterbore_diameter;
    let (counterbore_source, bore_source, carrier) = match (
        observed_cylinder_source_carrier(first_source, existing_geometries, counterbore_radius),
        observed_cylinder_source_carrier(second_source, existing_geometries, counterbore_radius),
    ) {
        (Some(carrier), None) => (first_source, second_source, carrier),
        (None, Some(carrier)) => (second_source, first_source, carrier),
        _ => return None,
    };
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        ..
    } = carrier
    else {
        return None;
    };
    let geometry = |radius| SurfaceGeometry::Cylinder {
        origin,
        axis,
        ref_direction,
        radius,
    };
    Some(
        counterbore_source
            .iter()
            .map(|id| (*id, geometry(counterbore_radius)))
            .chain(
                bore_source
                    .iter()
                    .map(|id| (*id, geometry(0.5 * bore_diameter))),
            )
            .collect(),
    )
}

pub(super) fn complete_cylinder_source_carrier(
    ids: &[u32],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    radius: f64,
) -> Option<SurfaceGeometry> {
    let carriers = ids
        .iter()
        .map(|id| existing_geometries.get(id))
        .collect::<Option<Vec<_>>>()?;
    let first = (*carriers.first()?).clone();
    (matches!(&first, SurfaceGeometry::Cylinder { radius: candidate, .. }
        if (*candidate - radius).abs() <= 1e-9)
        && carriers.iter().all(|candidate| **candidate == first))
    .then_some(first)
}

pub(super) fn observed_cylinder_source_carrier(
    ids: &[u32],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    radius: f64,
) -> Option<SurfaceGeometry> {
    let carriers = ids
        .iter()
        .filter_map(|id| existing_geometries.get(id))
        .filter(|geometry| {
            matches!(geometry, SurfaceGeometry::Cylinder { radius: candidate, .. }
                if (*candidate - radius).abs() <= 1e-9)
        })
        .collect::<Vec<_>>();
    let first = (*carriers.first()?).clone();
    carriers
        .iter()
        .all(|candidate| **candidate == first)
        .then_some(first)
}

pub(super) fn simple_hole_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<SimpleHoleGeometry> {
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
        .filter(|table| table.feature_id == Some(feature_id) && !table.surface_ids.is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [entry_plane, termination_plane, first_cylinder, second_cylinder] =
        table.entry_ids.as_slice()
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

pub(super) fn compact_simple_hole_cylinder_id(
    feature_id: u32,
    tables: &[crate::feature::FeatureEntityTable],
    rows: &[crate::surface::SurfaceRow],
) -> Option<u32> {
    let candidates = tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .filter_map(|table| {
            let entry_ids = table
                .entries
                .iter()
                .map(|entry| entry.entity_id)
                .collect::<Vec<_>>();
            (table.entry_ids == entry_ids).then_some(())?;

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
                        && class_204.source_entity_id.is_none()
                        && class_203.source_entity_id.is_none())
                    .then_some(())?;
                    let planes = pair
                        .iter()
                        .filter(|candidate| {
                            table.surface_ids.contains(&candidate.entity_id)
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
                            !table.surface_ids.contains(&candidate.entity_id)
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
                        && candidate.source_entity_id == Some(0)
                        && !table.surface_ids.contains(&candidate.entity_id)
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
                        && candidate.source_entity_id.is_none()
                        && table.surface_ids.contains(&candidate.entity_id)
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
            (table.surface_ids.iter().copied().collect::<BTreeSet<_>>() == expected_materialized
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

pub(super) fn compact_simple_hole_geometry(
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
        extent: Termination::Blind {
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

pub(super) fn circular_sweep_cylinder_from_cap_outlines(
    caps: [PartialCapOutline; 2],
) -> Option<SurfaceGeometry> {
    let (_, axis, _) = hole_placement(caps.map(|(id, origin, normal, _)| (id, origin, normal)))?;
    let axis_index = (0..3).find(|index| {
        axis[*index].abs() > 1.0 - 1e-9
            && (0..3).all(|other| other == *index || axis[other].abs() < 1e-9)
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
        radial
            .iter()
            .any(|index| (center[*index] - other_center[*index]).abs() > 1e-9 * scale)
            || (radius - other_radius).abs() > 1e-9 * scale
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
pub(super) struct CircularSweepGeometry {
    pub(super) cylinder_ids: Vec<u32>,
    pub(super) section_definition_id: Option<u32>,
    pub(super) direction: [f64; 3],
    pub(super) extent: ExtrudeExtent,
    pub(super) geometry: SurfaceGeometry,
}

pub(super) fn single_cap_circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && !table.surface_ids.is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [rowless_cap, cap_id, profile_id, cylinder_id] = table.entries.as_slice() else {
        return None;
    };
    ([
        rowless_cap.class_id,
        cap_id.class_id,
        profile_id.class_id,
        cylinder_id.class_id,
    ] == [204, 203, 200, 200]
        && profile_id.source_entity_id.is_some()
        && cylinder_id.source_entity_id.is_none()
        && table.surface_ids.contains(&cap_id.entity_id)
        && table.surface_ids.contains(&cylinder_id.entity_id)
        && table
            .non_surface_entity_ids
            .contains(&rowless_cap.entity_id)
        && table.non_surface_entity_ids.contains(&profile_id.entity_id))
    .then_some(())?;
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

pub(super) fn circular_sweep_feature_definition(
    profile: ProfileRef,
    sweep: &CircularSweepGeometry,
    op: BooleanOp,
    solid: Option<bool>,
) -> IrFeatureDefinition {
    IrFeatureDefinition::Extrude {
        profile,
        direction: cadmpeg_ir::features::ExtrudeDirection::Explicit(Vector3::new(
            sweep.direction[0],
            sweep.direction[1],
            sweep.direction[2],
        )),
        start: cadmpeg_ir::features::ExtrudeStart::default(),
        extent: sweep.extent.clone(),
        op,
        direction_source: None,
        solid,
        face_maker: None,
        inner_wire_taper: None,
        length_along_profile_normal: None,
        allow_multi_profile_faces: None,
    }
}

pub(super) fn circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    two_cap_circular_sweep_geometry(scan, feature_id)
        .or_else(|| single_cap_circular_sweep_geometry(scan, feature_id))
}

pub(super) fn two_cap_circular_sweep_geometry(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<CircularSweepGeometry> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && !table.surface_ids.is_empty())
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    let [first_plane_entry, second_plane_entry, profile_entry, cylinder_entry] =
        table.entries.as_slice()
    else {
        return None;
    };
    if table.entry_ids
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
        || first_plane_entry.source_entity_id.is_some()
        || second_plane_entry.source_entity_id.is_some()
        || profile_entry.source_entity_id.is_none()
        || cylinder_entry.source_entity_id.is_some()
        || !table.surface_ids.contains(&first_plane_entry.entity_id)
        || !table.surface_ids.contains(&second_plane_entry.entity_id)
        || !table.surface_ids.contains(&cylinder_entry.entity_id)
        || table.surface_ids.contains(&profile_entry.entity_id)
        || !table
            .non_surface_entity_ids
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
                offset: None,
            },
        },
        geometry: circular_sweep_cylinder_from_cap_outlines([cap(first), cap(second)])?,
    })
}

pub(super) fn extrusion_span(
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
        if (parallel / normal_length - 1.0).abs() > 1e-9 {
            continue;
        }
        let offset = origin
            .iter()
            .zip(profile_origin)
            .zip(direction)
            .map(|((coordinate, base), axis)| (coordinate - base) * axis)
            .sum::<f64>();
        if offset.abs() <= 1e-12 {
            continue;
        }
        let scale = offset.abs().max(1.0);
        if !offsets
            .iter()
            .any(|known| (known - offset).abs() <= 1e-9 * scale)
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

pub(super) fn extrusion_extent_and_direction(
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
    let extent = if (first - second).abs() <= 1e-9 * scale {
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

pub(super) fn blind_extrude_side(length: f64) -> ExtrudeSide {
    ExtrudeSide {
        termination: Termination::Blind {
            length: Length(length),
        },
        draft: None,
        offset: None,
    }
}
