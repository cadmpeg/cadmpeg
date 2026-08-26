// SPDX-License-Identifier: Apache-2.0
//! Counterbore dimensions, axis placement, and source cylinder geometry.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{Length, Termination};
use cadmpeg_ir::geometry::{CurveGeometry, SurfaceGeometry};
use cadmpeg_ir::ids::{CurveId, SurfaceId};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::container::ContainerScan;

use super::super::feature_history::{
    feature_dimension_table_complete, unique_surface_parameter_record,
};
use super::super::sketch::{approximately_equal, normalized};
use super::drilled::paired_corner_envelope_axis_spans;

const EPS_RADIUS_AGREEMENT: f64 = 1.0e-9;
const EPS_PARAMETER_DELTA: f64 = 1.0e-9;
const EPS_GEOMETRY_AGREEMENT: f64 = 1.0e-9;
const EPS_LENGTH_NONZERO: f64 = 1.0e-12;
const EPS_DEPTH_BOUND: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-9;

pub fn counterbore_dimensions(
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

pub fn counterbore_dimension_values<'a>(
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
                    <= EPS_RADIUS_AGREEMENT * radius.abs().max(counterbore_radius.abs()).max(1.0)
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
            .all(|delta| delta.abs() <= EPS_PARAMETER_DELTA)
        })
        .then_some(first)
}

pub fn counterbore_envelope_dimension_values<'a>(
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

pub fn counterbore_unenveloped_dimension_values<'a>(
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

pub fn counterbore_envelope_dimension_tuple(
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

pub fn unique_counterbore_dimension_tuple(
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

pub fn counterbore_patch_geometries(
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

pub fn counterbore_cylinder_sources(
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

pub fn counterbore_entity_table<'a>(
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

pub fn counterbore_axis_placement(
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

pub fn counterbore_support_axis_placement(
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

pub fn counterbore_axis_placement_from_sources(
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

pub fn counterbore_directed_placement(
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
pub struct CounterboreEnvelopeLayout {
    pub axis: usize,
    pub radial: [usize; 2],
    pub center: [f64; 3],
    pub axial_interval: [f64; 2],
}

pub fn counterbore_source_envelope_layout(
    corners: [[[f64; 3]; 2]; 2],
    diameter: f64,
    axial_depth: Option<f64>,
    scale: f64,
) -> Option<CounterboreEnvelopeLayout> {
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

pub fn counterbore_placement_from_corner_envelopes(
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
    let close = |left: f64, right: f64| (left - right).abs() <= EPS_GEOMETRY_AGREEMENT * scale;
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

pub fn counterbore_directed_span(
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
    (length.is_finite()
        && length > EPS_LENGTH_NONZERO * scale
        && counterbore_depth <= length + EPS_DEPTH_BOUND * scale)
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
            (alignment - 1.0).abs() <= EPS_AXIS_ALIGNMENT
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

pub fn counterbore_source_boundary_circle(
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
                ((*candidate - radius).abs() <= EPS_RADIUS_AGREEMENT).then_some(())?;
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
                ((alignment - 1.0).abs() <= EPS_AXIS_ALIGNMENT
                    && distance <= EPS_GEOMETRY_AGREEMENT * scale)
                    .then_some(())?;
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
                    >= 1.0 - EPS_AXIS_ALIGNMENT
        })
        .then_some(first)
}

pub fn counterbore_source_patch_geometries(
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

pub fn complete_cylinder_source_carrier(
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
        if (*candidate - radius).abs() <= EPS_RADIUS_AGREEMENT)
        && carriers.iter().all(|candidate| **candidate == first))
    .then_some(first)
}

pub fn observed_cylinder_source_carrier(
    ids: &[u32],
    existing_geometries: &BTreeMap<u32, SurfaceGeometry>,
    radius: f64,
) -> Option<SurfaceGeometry> {
    let carriers = ids
        .iter()
        .filter_map(|id| existing_geometries.get(id))
        .filter(|geometry| {
            matches!(geometry, SurfaceGeometry::Cylinder { radius: candidate, .. }
                if (*candidate - radius).abs() <= EPS_RADIUS_AGREEMENT)
        })
        .collect::<Vec<_>>();
    let first = (*carriers.first()?).clone();
    carriers
        .iter()
        .all(|candidate| **candidate == first)
        .then_some(first)
}
