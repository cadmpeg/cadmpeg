// SPDX-License-Identifier: Apache-2.0
//! Simple drilled-hole recipes, envelopes, and dimension matching.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::HoleForm;
use cadmpeg_ir::math::{Point3, Vector3};

use crate::container::ContainerScan;

use super::super::feature_history::{
    feature_dimension_table_complete, unique_surface_parameter_record,
};
use super::super::sketch::{approximately_equal, normalized};
use super::super::sweep::unique_available_positional_cylinder_frames;

const EPS_RADIUS_AGREEMENT: f64 = 1e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1e-9;
const EPS_COORDINATE_AGREEMENT: f64 = 1e-9;
const EPS_DIAMETER_NONZERO: f64 = 1e-12;
const EPS_GEOMETRY_AGREEMENT: f64 = 1e-9;

pub fn stepped_hole_form(
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

pub fn paired_hole_replay_surfaces_by_source(
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
pub enum SimpleDrilledDimensionFamily {
    ExternalId2Depth,
    ExternalId4Depth,
}

impl SimpleDrilledDimensionFamily {
    pub fn depth_external_id(self) -> u32 {
        match self {
            Self::ExternalId2Depth => 2,
            Self::ExternalId4Depth => 4,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SimpleDrilledHoleRecipe<'a> {
    pub table: &'a crate::feature::FeatureEntityTable,
    pub dimension_family: SimpleDrilledDimensionFamily,
}

pub fn simple_drilled_hole_recipe<'a>(
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

pub fn simple_drilled_hole_envelope_spans(
    scan: &ContainerScan,
    table: &crate::feature::FeatureEntityTable,
) -> Option<[[Option<f64>; 2]; 3]> {
    let [first, second] = simple_drilled_hole_corner_envelopes(scan, table)?;
    paired_corner_envelope_axis_spans(first, second)
}

pub fn simple_drilled_hole_corner_envelopes(
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

pub fn simple_drilled_hole_cone_terminal_points(
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

pub fn simple_drilled_hole_placement(
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

pub fn simple_drilled_hole_axis_placement(
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

pub fn simple_drilled_axis_placement_from_frames(
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
                || (frame.radius - radius).abs() > EPS_RADIUS_AGREEMENT * radius_scale
            {
                return false;
            }
            let alignment = axis
                .into_iter()
                .zip(candidate_axis)
                .map(|(left, right)| left * right)
                .sum::<f64>();
            if alignment.abs() < 1.0 - EPS_AXIS_ALIGNMENT {
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
                <= EPS_COORDINATE_AGREEMENT * coordinate_scale
        })
        .then_some(cadmpeg_ir::features::HolePlacement::Axis {
            origin: Point3::new(first.origin[0], first.origin[1], first.origin[2]),
            axis: Vector3::new(axis[0], axis[1], axis[2]),
        })
}

#[derive(Debug, Clone, Copy)]
pub struct DrilledHoleEnvelopeLayout {
    pub corners: [[[f64; 3]; 2]; 2],
    pub intervals: [[[f64; 2]; 3]; 2],
    pub axis: usize,
    pub radial: [usize; 2],
    pub axial_delta: f64,
    pub scale: f64,
}

pub fn drilled_hole_envelope_layout(
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
    (diameter > EPS_DIAMETER_NONZERO * scale && depth > EPS_DIAMETER_NONZERO * scale)
        .then_some(())?;
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
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

pub fn drilled_hole_layout_close(
    layout: &DrilledHoleEnvelopeLayout,
    left: f64,
    right: f64,
) -> bool {
    (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * layout.scale
}

pub fn drilled_hole_layout_shared(layout: &DrilledHoleEnvelopeLayout, axis: usize) -> bool {
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

pub fn drilled_hole_layout_adjacent(layout: &DrilledHoleEnvelopeLayout, axis: usize) -> bool {
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

pub fn drilled_hole_layout_span(layout: &DrilledHoleEnvelopeLayout, axis: usize) -> f64 {
    layout.intervals[0][axis][1].max(layout.intervals[1][axis][1])
        - layout.intervals[0][axis][0].min(layout.intervals[1][axis][0])
}

pub fn drilled_hole_layout_placement(
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

pub fn drilled_hole_placement_from_corner_envelopes(
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

pub fn clipped_drilled_hole_placement_from_cone_points(
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

pub fn paired_corner_envelope_axis_spans(
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

pub fn simple_drilled_hole_dimensions(
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

pub fn simple_drilled_hole_dimension_values<'a>(
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

pub fn dimension_pair_matches_envelope_spans(
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
