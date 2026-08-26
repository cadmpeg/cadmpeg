// SPDX-License-Identifier: Apache-2.0
//! Extrusion span resolution from carriers, cylinders, NURBS translation, and rectilinear planes.

use super::super::analytic::{canonical_plane, dot, PlaneEquation};
use super::super::holes::{extrusion_span, ExtrusionSpan};
use super::super::sketch::normalized;
use super::planes::{
    feature_plane_equations, generated_arc_cylinder_extent, generated_cap_plane_extent,
};
use crate::container::ContainerScan;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::{ExtrudeExtent, ExtrudeSide, Length, Termination};
use cadmpeg_ir::geometry::{NurbsSurface, Surface, SurfaceGeometry};
use cadmpeg_ir::ids::SurfaceId;

const EPS_COORDINATE_AGREEMENT: f64 = 1.0e-9;
const EPS_VECTOR_AGREEMENT: f64 = 1.0e-9;
const EPS_AXIS_ALIGNMENT: f64 = 1.0e-10;
const EPS_WEIGHT_AGREEMENT: f64 = 1.0e-10;
const EPS_STATION_AGREEMENT: f64 = 1.0e-9;

pub(in super::super) struct ExtrusionCarrierSpan {
    pub(in super::super) starts: Vec<[f64; 3]>,
    pub(in super::super) vector: [f64; 3],
}

pub(in super::super) fn blind_extrusion_from_carriers(
    carriers: &[ExtrusionCarrierSpan],
    planes: &[([f64; 3], [f64; 3])],
    transform: Option<&crate::placement::FeatureSectionTransform>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let first = carriers.first()?;
    let first_start = *first.starts.first()?;
    let direction = normalized(first.vector)?;
    let length = first.vector.into_iter().fold(0.0_f64, f64::hypot);
    (length.is_finite() && length > 0.0).then_some(())?;
    let coordinate_scale = carriers
        .iter()
        .flat_map(|carrier| carrier.starts.iter().flatten().copied())
        .chain(planes.iter().flat_map(|(origin, _)| *origin))
        .chain(transform.into_iter().flat_map(|transform| transform.origin))
        .map(f64::abs)
        .fold(length.max(1.0), f64::max);
    let tolerance = EPS_COORDINATE_AGREEMENT * coordinate_scale;
    let vector_tolerance = EPS_VECTOR_AGREEMENT * length.max(1.0);
    let start_station = dot(first_start, direction);
    let end_station = start_station + length;
    let mut has_opposed_carrier = false;
    carriers
        .iter()
        .all(|carrier| {
            if carrier.starts.is_empty() {
                return false;
            }
            let same_direction = carrier
                .vector
                .into_iter()
                .zip(first.vector)
                .all(|(candidate, reference)| (candidate - reference).abs() <= vector_tolerance);
            let opposite_direction = carrier
                .vector
                .into_iter()
                .zip(first.vector)
                .all(|(candidate, reference)| (candidate + reference).abs() <= vector_tolerance);
            has_opposed_carrier |= opposite_direction;
            (same_direction
                && carrier
                    .starts
                    .iter()
                    .all(|start| (dot(*start, direction) - start_station).abs() <= tolerance))
                || (opposite_direction
                    && carrier
                        .starts
                        .iter()
                        .all(|start| (dot(*start, direction) - end_station).abs() <= tolerance))
        })
        .then_some(())?;
    let cap_stations = planes
        .iter()
        .map(|(origin, normal)| {
            let normal = normalized(*normal)?;
            let alignment = dot(normal, direction).abs();
            if alignment >= 1.0 - EPS_AXIS_ALIGNMENT {
                Some(Some(dot(*origin, direction)))
            } else if alignment <= EPS_AXIS_ALIGNMENT {
                Some(None)
            } else {
                None
            }
        })
        .collect::<Option<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut unique_stations = Vec::new();
    for station in cap_stations {
        if unique_stations
            .iter()
            .all(|existing| (station - existing).abs() > tolerance)
        {
            unique_stations.push(station);
        }
    }
    let reverse = if has_opposed_carrier {
        if let Some(transform) = transform {
            let transform_station = dot(transform.origin, direction);
            if (transform_station - start_station).abs() <= tolerance {
                false
            } else if (transform_station - end_station).abs() <= tolerance {
                true
            } else {
                return None;
            }
        } else {
            let [terminal_station] = unique_stations.as_slice() else {
                return None;
            };
            if (*terminal_station - end_station).abs() <= tolerance {
                false
            } else if (*terminal_station - start_station).abs() <= tolerance {
                true
            } else {
                return None;
            }
        }
    } else {
        false
    };
    let (direction, start_station, end_station) = if reverse {
        (
            direction.map(|component| -component),
            -end_station,
            -start_station,
        )
    } else {
        (direction, start_station, end_station)
    };
    if let Some(transform) = transform {
        let normal = normalized(transform.normal)?;
        ((dot(direction, normal).abs() - 1.0).abs() <= EPS_AXIS_ALIGNMENT
            && (dot(transform.origin, direction) - start_station).abs() <= tolerance)
            .then_some(())?;
    }
    let unique_stations = unique_stations
        .into_iter()
        .map(|station| if reverse { -station } else { station })
        .collect::<Vec<_>>();
    let cap_matches = |cap: f64| {
        (cap - start_station).abs() <= tolerance || (cap - end_station).abs() <= tolerance
    };
    match unique_stations.as_slice() {
        [] => {}
        [cap] if cap_matches(*cap) => {}
        [first_cap, second_cap]
            if cap_matches(*first_cap)
                && cap_matches(*second_cap)
                && ((first_cap - second_cap).abs() - length).abs() <= tolerance => {}
        _ => return None,
    }
    Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(length),
                },
                draft: None,
                offset: None,
            },
        },
        direction,
    ))
}

pub(in super::super) fn generated_bounded_cylinder_extent(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    transform: Option<&crate::placement::FeatureSectionTransform>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!rows.is_empty()
        && rows.iter().all(|row| {
            matches!(
                row.kind,
                crate::surface::SurfaceKind::Plane | crate::surface::SurfaceKind::Cylinder
            )
        }))
    .then_some(())?;

    let mut frames = Vec::new();
    let mut planes = Vec::new();
    for row in rows {
        (crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(row))
            .then_some(())?;
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .collect::<Vec<_>>();
        match row.kind {
            crate::surface::SurfaceKind::Plane => match surfaces.as_slice() {
                [] => {}
                [Surface {
                    geometry: SurfaceGeometry::Plane { origin, normal, .. },
                    ..
                }] => planes.push((
                    [origin.x, origin.y, origin.z],
                    [normal.x, normal.y, normal.z],
                )),
                _ => return None,
            },
            crate::surface::SurfaceKind::Cylinder => {
                let [Surface {
                    geometry: SurfaceGeometry::Cylinder { origin, axis, .. },
                    ..
                }] = surfaces.as_slice()
                else {
                    return None;
                };
                let parameters =
                    crate::surface::unique_surface_parameter(&scan.surfaces.parameters, row.id)?;
                let frame = parameters.positional_cylinder_frame?;
                let transferred_origin = [origin.x, origin.y, origin.z];
                let transferred_axis = normalized([axis.x, axis.y, axis.z])?;
                let frame_axis = normalized(frame.axis)?;
                let scale = transferred_origin
                    .into_iter()
                    .chain(frame.origin)
                    .map(f64::abs)
                    .fold(1.0, f64::max);
                (transferred_origin
                    .into_iter()
                    .zip(frame.origin)
                    .all(|(left, right)| (left - right).abs() <= EPS_COORDINATE_AGREEMENT * scale)
                    && transferred_axis
                        .into_iter()
                        .zip(frame_axis)
                        .all(|(left, right)| (left - right).abs() <= EPS_WEIGHT_AGREEMENT))
                .then_some(())?;
                frames.push(frame);
            }
            _ => unreachable!("surface family checked above"),
        }
    }
    let carriers = frames
        .into_iter()
        .map(|frame| bounded_cylinder_span(frame, &planes))
        .collect::<Option<Vec<_>>>()?;
    blind_extrusion_from_carriers(&carriers, &planes, transform)
}

pub(in super::super) fn bounded_cylinder_span(
    frame: crate::surface::PositionalCylinderFrame,
    planes: &[([f64; 3], [f64; 3])],
) -> Option<ExtrusionCarrierSpan> {
    let axis = normalized(frame.axis)?;
    let vector = match frame.length {
        Some(length) => {
            (length.is_finite() && length > 0.0).then_some(())?;
            axis.map(|component| component * length)
        }
        None => {
            let scale = planes
                .iter()
                .flat_map(|(origin, _)| *origin)
                .chain(frame.origin)
                .map(f64::abs)
                .fold(1.0, f64::max);
            let tolerance = EPS_COORDINATE_AGREEMENT * scale;
            let start_station = dot(frame.origin, axis);
            let mut terminal_offsets = Vec::new();
            for (origin, normal) in planes {
                let normal = normalized(*normal)?;
                let alignment = dot(normal, axis).abs();
                if alignment >= 1.0 - EPS_AXIS_ALIGNMENT {
                    let offset = dot(*origin, axis) - start_station;
                    if offset.abs() > tolerance
                        && terminal_offsets
                            .iter()
                            .all(|existing| (offset - existing).abs() > tolerance)
                    {
                        terminal_offsets.push(offset);
                    }
                } else if alignment > EPS_AXIS_ALIGNMENT {
                    return None;
                }
            }
            let [offset] = terminal_offsets.as_slice() else {
                return None;
            };
            axis.map(|component| component * offset)
        }
    };
    Some(ExtrusionCarrierSpan {
        starts: vec![frame.origin],
        vector,
    })
}

pub(in super::super) fn nurbs_translation_candidate(
    nurbs: &NurbsSurface,
    along_v: bool,
) -> Option<ExtrusionCarrierSpan> {
    let (degree, count, knots, periodic) = if along_v {
        (
            nurbs.v_degree,
            nurbs.v_count,
            nurbs.v_knots.as_slice(),
            nurbs.v_periodic,
        )
    } else {
        (
            nurbs.u_degree,
            nurbs.u_count,
            nurbs.u_knots.as_slice(),
            nurbs.u_periodic,
        )
    };
    let [first, second, third, fourth] = knots else {
        return None;
    };
    (degree == 1
        && count == 2
        && !periodic
        && first.is_finite()
        && fourth.is_finite()
        && first == second
        && third == fourth
        && first < third)
        .then_some(())?;
    let u_count = usize::try_from(nurbs.u_count).ok()?;
    let v_count = usize::try_from(nurbs.v_count).ok()?;
    (u_count.checked_mul(v_count)? == nurbs.control_points.len()
        && nurbs
            .weights
            .as_ref()
            .is_none_or(|weights| weights.len() == nurbs.control_points.len()))
    .then_some(())?;
    let pair_count = if along_v { u_count } else { v_count };
    let mut starts = Vec::with_capacity(pair_count);
    let mut vector: Option<[f64; 3]> = None;
    for index in 0..pair_count {
        let (start_index, end_index) = if along_v {
            (index * v_count, index * v_count + 1)
        } else {
            (index, v_count + index)
        };
        let start = *nurbs.control_points.get(start_index)?;
        let end = *nurbs.control_points.get(end_index)?;
        let start = [start.x, start.y, start.z];
        let end = [end.x, end.y, end.z];
        start
            .into_iter()
            .chain(end)
            .all(f64::is_finite)
            .then_some(())?;
        if let Some(weights) = &nurbs.weights {
            let start_weight = *weights.get(start_index)?;
            let end_weight = *weights.get(end_index)?;
            (start_weight.is_finite()
                && end_weight.is_finite()
                && (start_weight - end_weight).abs()
                    <= EPS_WEIGHT_AGREEMENT * start_weight.abs().max(end_weight.abs()).max(1.0))
            .then_some(())?;
        }
        let candidate = std::array::from_fn(|axis| end[axis] - start[axis]);
        if let Some(reference) = vector {
            let scale = reference
                .into_iter()
                .chain(candidate)
                .map(f64::abs)
                .fold(1.0, f64::max);
            candidate
                .into_iter()
                .zip(reference)
                .all(|(left, right)| (left - right).abs() <= EPS_COORDINATE_AGREEMENT * scale)
                .then_some(())?;
        } else {
            vector = Some(candidate);
        }
        starts.push(start);
    }
    Some(ExtrusionCarrierSpan {
        starts,
        vector: vector?,
    })
}

pub(in super::super) fn nurbs_translation_span(
    nurbs: &NurbsSurface,
) -> Option<ExtrusionCarrierSpan> {
    let mut candidates = [true, false]
        .into_iter()
        .filter_map(|along_v| nurbs_translation_candidate(nurbs, along_v));
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(in super::super) fn generated_nurbs_translation_extent(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    transform: Option<&crate::placement::FeatureSectionTransform>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (!rows.is_empty()
        && rows.iter().all(|row| {
            matches!(
                row.kind,
                crate::surface::SurfaceKind::Plane | crate::surface::SurfaceKind::Extrusion
            )
        }))
    .then_some(())?;
    let mut carriers = Vec::new();
    let mut planes = Vec::new();
    for row in rows {
        (crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(row))
            .then_some(())?;
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .collect::<Vec<_>>();
        match row.kind {
            crate::surface::SurfaceKind::Plane => match surfaces.as_slice() {
                [] => {}
                [Surface {
                    geometry: SurfaceGeometry::Plane { origin, normal, .. },
                    ..
                }] => planes.push((
                    [origin.x, origin.y, origin.z],
                    [normal.x, normal.y, normal.z],
                )),
                _ => return None,
            },
            crate::surface::SurfaceKind::Extrusion => match surfaces.as_slice() {
                [] => {}
                [Surface {
                    geometry: SurfaceGeometry::Nurbs(nurbs),
                    ..
                }] => carriers.push(nurbs_translation_span(nurbs)?),
                _ => return None,
            },
            _ => unreachable!("surface family checked above"),
        }
    }
    blind_extrusion_from_carriers(&carriers, &planes, transform)
}

pub(in super::super) struct RectilinearPlaneStation {
    pub(in super::super) coordinate: f64,
    pub(in super::super) reversed: bool,
}

pub(in super::super) struct RectilinearPlaneFamily {
    pub(in super::super) normal: [f64; 3],
    pub(in super::super) stations: Vec<RectilinearPlaneStation>,
}

pub(in super::super) fn rectilinear_family_extent(
    family: &RectilinearPlaneFamily,
    start_reversed: bool,
    station_tolerance: f64,
) -> Option<([f64; 3], f64)> {
    let first = family
        .stations
        .iter()
        .min_by(|left, right| left.coordinate.total_cmp(&right.coordinate))?;
    let last = family
        .stations
        .iter()
        .max_by(|left, right| left.coordinate.total_cmp(&right.coordinate))?;
    (first.coordinate.is_finite()
        && last.coordinate.is_finite()
        && (last.coordinate - first.coordinate).abs() > station_tolerance)
        .then_some(())?;
    let (start, end) = if first.reversed == start_reversed && last.reversed != start_reversed {
        (first.coordinate, last.coordinate)
    } else if last.reversed == start_reversed && first.reversed != start_reversed {
        (last.coordinate, first.coordinate)
    } else {
        return None;
    };
    let signed_length = end - start;
    let direction = if first.reversed == start_reversed {
        family.normal
    } else {
        family.normal.map(|component| -component)
    };
    (signed_length.abs() > station_tolerance).then_some((direction, signed_length.abs()))
}

pub(in super::super) fn generated_rectilinear_plane_extent(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
    section: Option<&crate::feature::FeatureSection3d>,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let section = section?;
    section.sketch_plane_entity_id?;
    let plane_flip = section.sketch_plane_flip == Some(crate::feature::BinaryFlag::Set);
    let section_flip = section.orientation.section_flip == Some(crate::feature::BinaryFlag::Set);
    let start_reversed = plane_flip ^ section_flip;
    let rows = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| row.feature_id == feature_id)
        .collect::<Vec<_>>();
    (rows.len() >= 4
        && rows
            .iter()
            .all(|row| row.kind == crate::surface::SurfaceKind::Plane))
    .then_some(())?;

    let mut planes = Vec::with_capacity(rows.len());
    for row in rows {
        (crate::surface::unique_surface_row(&scan.surfaces.rows, row.id) == Some(row))
            .then_some(())?;
        let id = SurfaceId(format!("creo:visibgeom:surface#{}", row.id));
        let surfaces = ir
            .model
            .surfaces
            .iter()
            .filter(|surface| surface.id == id)
            .collect::<Vec<_>>();
        let [Surface {
            geometry: SurfaceGeometry::Plane { origin, normal, .. },
            ..
        }] = surfaces.as_slice()
        else {
            return None;
        };
        let plane = canonical_plane(PlaneEquation {
            origin: [origin.x, origin.y, origin.z],
            normal: [normal.x, normal.y, normal.z],
        })?;
        planes.push((plane, row.reversed));
    }

    let coordinate_scale = planes
        .iter()
        .flat_map(|(plane, _)| plane.origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    let station_tolerance = EPS_STATION_AGREEMENT * coordinate_scale;
    let mut families: Vec<RectilinearPlaneFamily> = Vec::new();
    for (plane, reversed) in planes {
        let station = dot(plane.origin, plane.normal);
        station.is_finite().then_some(())?;
        if let Some(family) = families.iter_mut().find(|family| {
            family
                .normal
                .iter()
                .zip(plane.normal)
                .all(|(left, right)| (left - right).abs() <= EPS_WEIGHT_AGREEMENT)
        }) {
            if let Some(known) = family
                .stations
                .iter()
                .find(|known| (station - known.coordinate).abs() <= station_tolerance)
            {
                (known.reversed == reversed).then_some(())?;
            } else {
                family.stations.push(RectilinearPlaneStation {
                    coordinate: station,
                    reversed,
                });
            }
        } else {
            families
                .iter()
                .all(|family| dot(family.normal, plane.normal).abs() <= EPS_AXIS_ALIGNMENT)
                .then_some(())?;
            families.push(RectilinearPlaneFamily {
                normal: plane.normal,
                stations: vec![RectilinearPlaneStation {
                    coordinate: station,
                    reversed,
                }],
            });
        }
    }
    (families.len() >= 2
        && families
            .iter()
            .filter(|family| family.stations.len() >= 2)
            .count()
            >= 2)
        .then_some(())?;

    let candidates = families
        .iter()
        .filter_map(|family| {
            let (direction, length) =
                rectilinear_family_extent(family, start_reversed, station_tolerance)?;
            Some((direction.map(|component| component * length), length))
        })
        .collect::<Vec<_>>();
    let [(vector, length)] = candidates.as_slice() else {
        return None;
    };
    let direction = normalized(*vector)?;
    Some((
        ExtrudeExtent::OneSided {
            side: ExtrudeSide {
                termination: Termination::Blind {
                    length: Length(*length),
                },
                draft: None,
                offset: None,
            },
        },
        direction,
    ))
}

pub(in super::super) fn directed_blind_extrusion_span(
    profile_direction: [f64; 3],
    extrusion_direction: [f64; 3],
    length: f64,
) -> Option<ExtrusionSpan> {
    (length.is_finite() && length > 0.0).then_some(())?;
    let profile_direction = normalized(profile_direction)?;
    let extrusion_direction = normalized(extrusion_direction)?;
    let alignment = dot(profile_direction, extrusion_direction);
    (alignment.abs() >= 1.0 - EPS_COORDINATE_AGREEMENT).then_some(())?;
    Some(if alignment.is_sign_positive() {
        ExtrusionSpan {
            lower: 0.0,
            upper: length,
        }
    } else {
        ExtrusionSpan {
            lower: -length,
            upper: 0.0,
        }
    })
}

pub(in super::super) fn feature_id_for_section_transform(
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<u32> {
    match (definition.owner_feature_id, transform.feature_id) {
        (Some(definition_feature_id), Some(transform_feature_id))
            if definition_feature_id != transform_feature_id =>
        {
            None
        }
        (Some(feature_id), _) | (_, Some(feature_id)) => Some(feature_id),
        (None, None) => None,
    }
}

pub(in super::super) fn derived_blind_extrusion_span(
    transform: &crate::placement::FeatureSectionTransform,
    extent: &ExtrudeExtent,
    direction: [f64; 3],
) -> Option<ExtrusionSpan> {
    let ExtrudeExtent::OneSided {
        side:
            ExtrudeSide {
                termination: Termination::Blind { length },
                ..
            },
    } = extent
    else {
        return None;
    };
    directed_blind_extrusion_span(transform.normal, direction, length.0)
}

pub(in super::super) fn resolved_feature_extrusion_span(
    scan: &ContainerScan,
    ir: &CadIr,
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<ExtrusionSpan> {
    let feature_id = feature_id_for_section_transform(definition, transform)?;
    generated_arc_cylinder_extent(scan, definition, transform)
        .and_then(|(extent, direction)| derived_blind_extrusion_span(transform, &extent, direction))
        .or_else(|| {
            feature_plane_equations(scan, feature_id)
                .and_then(|planes| extrusion_span(transform.origin, transform.normal, planes))
        })
        .or_else(|| {
            generated_cap_plane_extent(scan, ir, feature_id).and_then(|(extent, direction)| {
                derived_blind_extrusion_span(transform, &extent, direction)
            })
        })
        .or_else(|| {
            generated_bounded_cylinder_extent(scan, ir, feature_id, Some(transform)).and_then(
                |(extent, direction)| derived_blind_extrusion_span(transform, &extent, direction),
            )
        })
        .or_else(|| {
            generated_nurbs_translation_extent(scan, ir, feature_id, Some(transform)).and_then(
                |(extent, direction)| derived_blind_extrusion_span(transform, &extent, direction),
            )
        })
}
