// SPDX-License-Identifier: Apache-2.0
//! Extrusion and revolution surface and B-rep transfer from resolved sketches.

#[allow(clippy::wildcard_imports)]
use super::*;

pub(super) fn feature_plane_equations(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<Vec<([f64; 3], [f64; 3])>> {
    let ids = scan
        .surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .collect::<BTreeSet<_>>();
    ids.into_iter()
        .map(|id| {
            crate::surface::unique_surface_row(&scan.surfaces.rows, id)?;
            let outlines = scan
                .planes
                .outlines
                .iter()
                .filter(|plane| plane.surface_id == id)
                .collect::<Vec<_>>();
            match outlines.as_slice() {
                [plane] => Some((plane.origin, plane.normal)),
                [] => {
                    let frames = scan
                        .planes
                        .local_systems
                        .iter()
                        .filter(|frame| frame.surface_id == id)
                        .collect::<Vec<_>>();
                    let [frame] = frames.as_slice() else {
                        return None;
                    };
                    Some((frame.origin?, frame.normal?))
                }
                _ => None,
            }
        })
        .collect()
}

pub(super) type FeatureOutlinePlane = (u32, [f64; 3], [f64; 3]);

/// Resolve one uniquely identified plane row to one unambiguous placed plane
/// equation. The equation may be carried by one outline and one positional
/// frame; both carriers are valid only when they agree on origin and normal.
pub(super) fn feature_outline_plane(
    scan: &ContainerScan,
    feature_id: u32,
    surface_id: u32,
) -> Option<FeatureOutlinePlane> {
    let row = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)?;
    (row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane)
        .then_some(())?;
    let outlines = scan
        .planes
        .outlines
        .iter()
        .filter(|plane| plane.surface_id == surface_id);
    let outlines = outlines.cloned().collect::<Vec<_>>();
    let positional_frames = scan
        .planes
        .positional_frames
        .iter()
        .filter(|plane| plane.surface_id == surface_id)
        .cloned()
        .collect::<Vec<_>>();
    let agrees = |left: &crate::surface::OutlinePlane, right: &crate::surface::OutlinePlane| {
        left.origin
            .into_iter()
            .zip(right.origin)
            .all(|(left, right)| {
                (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
            })
            && left
                .normal
                .into_iter()
                .zip(right.normal)
                .all(|(left, right)| {
                    (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0)
                })
    };
    let plane = match (outlines.as_slice(), positional_frames.as_slice()) {
        ([], []) => return None,
        ([plane], []) | ([], [plane]) => plane,
        ([outline], [positional]) if agrees(outline, positional) => outline,
        _ => return None,
    };
    Some((surface_id, plane.origin, plane.normal))
}

/// Collect every same-feature plane row only when all rows have complete,
/// unambiguous placed equations. Partial collections cannot establish ordered
/// caps.
pub(super) fn feature_outline_planes(
    scan: &ContainerScan,
    feature_id: u32,
) -> Option<Vec<FeatureOutlinePlane>> {
    scan.surfaces
        .rows
        .iter()
        .filter(|row| {
            row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane
        })
        .map(|row| row.id)
        .map(|id| feature_outline_plane(scan, feature_id, id))
        .collect()
}

pub(super) fn generated_arc_cylinder_extent(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
    transform: &crate::placement::FeatureSectionTransform,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let feature_id = definition.owner_feature_id?;
    definition.segments.as_ref()?.is_complete().then_some(())?;
    let mut surface_ids = BTreeSet::new();
    for (_, entry) in scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| table.entries.iter().map(move |entry| (table, entry)))
        .filter(|(table, entry)| {
            entry.class_id == 200 && table.surface_ids.contains(&entry.entity_id)
        })
    {
        let Some(source_id) = entry.source_entity_id else {
            continue;
        };
        let Some(segment) = definition.segments.as_ref()?.segment(source_id) else {
            continue;
        };
        if segment.kind != crate::feature::FeatureSegmentKind::Arc {
            continue;
        }
        let Some(row) = crate::surface::unique_surface_row(&scan.surfaces.rows, entry.entity_id)
            .filter(|row| {
                row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Cylinder
            })
        else {
            continue;
        };
        surface_ids.insert(row.id).then_some(())?;
    }
    let frames =
        unique_available_positional_cylinder_frames(&surface_ids, &scan.surfaces.parameters)?;
    (!frames.is_empty()).then_some(())?;
    agreed_generated_cylinder_extent(transform, &frames)
}

pub(super) fn ordered_parallel_cap_extent(
    start: PlaneEquation,
    end: PlaneEquation,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let start = canonical_plane(start)?;
    let end = canonical_plane(end)?;
    start
        .normal
        .into_iter()
        .zip(end.normal)
        .all(|(left, right)| (left - right).abs() <= 1e-10)
        .then_some(())?;
    let signed_length = dot(
        std::array::from_fn(|axis| end.origin[axis] - start.origin[axis]),
        start.normal,
    );
    let scale = start
        .origin
        .into_iter()
        .chain(end.origin)
        .map(f64::abs)
        .fold(1.0, f64::max);
    (signed_length.abs() > 1e-9 * scale).then_some(())?;
    Some((
        ExtrudeExtent::OneSided {
            side: blind_extrude_side(signed_length.abs()),
        },
        start
            .normal
            .map(|component| component * signed_length.signum()),
    ))
}

pub(super) fn generated_cap_plane_extent(
    scan: &ContainerScan,
    ir: &CadIr,
    feature_id: u32,
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let tables = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id) && table.table_class_id == 29)
        .collect::<Vec<_>>();
    let [table] = tables.as_slice() else {
        return None;
    };
    table
        .entries
        .iter()
        .map(|entry| entry.entity_id)
        .eq(table.entry_ids.iter().copied())
        .then_some(())?;
    let mut start_id = None;
    let mut end_id = None;
    let mut side_count = 0_usize;
    for entry in &table.entries {
        match (entry.class_id, entry.source_entity_id) {
            (204, None) if start_id.replace(entry.entity_id).is_none() => {}
            (203, None) if end_id.replace(entry.entity_id).is_none() => {}
            (200, Some(_)) => side_count += 1,
            _ => return None,
        }
    }
    (side_count > 0
        && table.surface_ids.contains(&start_id?)
        && table.surface_ids.contains(&end_id?))
    .then_some(())?;
    let plane = |surface_id: u32| {
        let row = crate::surface::unique_surface_row(&scan.surfaces.rows, surface_id)?;
        (row.feature_id == feature_id && row.kind == crate::surface::SurfaceKind::Plane)
            .then_some(())?;
        let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
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
        Some(PlaneEquation {
            origin: [origin.x, origin.y, origin.z],
            normal: [normal.x, normal.y, normal.z],
        })
    };
    ordered_parallel_cap_extent(plane(start_id?)?, plane(end_id?)?)
}

pub(super) fn unique_available_positional_cylinder_frames(
    surface_ids: &BTreeSet<u32>,
    parameters: &[crate::surface::SurfaceParameterRecord],
) -> Option<Vec<crate::surface::PositionalCylinderFrame>> {
    let mut frames = Vec::new();
    for surface_id in surface_ids {
        let mut matching = parameters
            .iter()
            .filter(|record| record.surface_id == *surface_id);
        let first = matching.next();
        if matching.next().is_some() {
            return None;
        }
        if let Some(frame) = first.and_then(|record| record.positional_cylinder_frame) {
            frames.push(frame);
        }
    }
    Some(frames)
}

pub(super) fn agreed_generated_cylinder_extent(
    transform: &crate::placement::FeatureSectionTransform,
    frames: &[crate::surface::PositionalCylinderFrame],
) -> Option<(ExtrudeExtent, [f64; 3])> {
    let normal = normalized(transform.normal)?;
    let first = *frames.first()?;
    let length = first.length.filter(|length| *length > 0.0)?;
    let direction = normalized(first.axis)?;
    let close =
        |left: f64, right: f64| (left - right).abs() <= 1e-9 * left.abs().max(right.abs()).max(1.0);
    frames
        .iter()
        .all(|frame| {
            frame
                .length
                .is_some_and(|candidate| close(candidate, length))
                && normalized(frame.axis).is_some_and(|axis| {
                    axis.iter()
                        .zip(direction)
                        .all(|(left, right)| close(*left, right))
                })
                && close(
                    dot(
                        std::array::from_fn(|index| frame.origin[index] - transform.origin[index]),
                        normal,
                    ),
                    0.0,
                )
        })
        .then_some(())?;
    close(dot(direction, normal).abs(), 1.0).then_some(())?;
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

pub(super) struct ExtrusionCarrierSpan {
    pub(super) starts: Vec<[f64; 3]>,
    pub(super) vector: [f64; 3],
}

pub(super) fn blind_extrusion_from_carriers(
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
    let tolerance = 1e-9 * coordinate_scale;
    let vector_tolerance = 1e-9 * length.max(1.0);
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
            if alignment >= 1.0 - 1e-10 {
                Some(Some(dot(*origin, direction)))
            } else if alignment <= 1e-10 {
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
        ((dot(direction, normal).abs() - 1.0).abs() <= 1e-10
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

pub(super) fn generated_bounded_cylinder_extent(
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
                    .all(|(left, right)| (left - right).abs() <= 1e-9 * scale)
                    && transferred_axis
                        .into_iter()
                        .zip(frame_axis)
                        .all(|(left, right)| (left - right).abs() <= 1e-10))
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

pub(super) fn bounded_cylinder_span(
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
            let tolerance = 1e-9 * scale;
            let start_station = dot(frame.origin, axis);
            let mut terminal_offsets = Vec::new();
            for (origin, normal) in planes {
                let normal = normalized(*normal)?;
                let alignment = dot(normal, axis).abs();
                if alignment >= 1.0 - 1e-10 {
                    let offset = dot(*origin, axis) - start_station;
                    if offset.abs() > tolerance
                        && terminal_offsets
                            .iter()
                            .all(|existing| (offset - existing).abs() > tolerance)
                    {
                        terminal_offsets.push(offset);
                    }
                } else if alignment > 1e-10 {
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

pub(super) fn nurbs_translation_candidate(
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
                    <= 1e-10 * start_weight.abs().max(end_weight.abs()).max(1.0))
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
                .all(|(left, right)| (left - right).abs() <= 1e-9 * scale)
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

pub(super) fn nurbs_translation_span(nurbs: &NurbsSurface) -> Option<ExtrusionCarrierSpan> {
    let mut candidates = [true, false]
        .into_iter()
        .filter_map(|along_v| nurbs_translation_candidate(nurbs, along_v));
    let candidate = candidates.next()?;
    candidates.next().is_none().then_some(candidate)
}

pub(super) fn generated_nurbs_translation_extent(
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

pub(super) struct RectilinearPlaneStation {
    pub(super) coordinate: f64,
    pub(super) reversed: bool,
}

pub(super) struct RectilinearPlaneFamily {
    pub(super) normal: [f64; 3],
    pub(super) stations: Vec<RectilinearPlaneStation>,
}

pub(super) fn rectilinear_family_extent(
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

pub(super) fn generated_rectilinear_plane_extent(
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
    let station_tolerance = 1e-9 * coordinate_scale;
    let mut families: Vec<RectilinearPlaneFamily> = Vec::new();
    for (plane, reversed) in planes {
        let station = dot(plane.origin, plane.normal);
        station.is_finite().then_some(())?;
        if let Some(family) = families.iter_mut().find(|family| {
            family
                .normal
                .iter()
                .zip(plane.normal)
                .all(|(left, right)| (left - right).abs() <= 1e-10)
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
                .all(|family| dot(family.normal, plane.normal).abs() <= 1e-10)
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

pub(super) fn directed_blind_extrusion_span(
    profile_direction: [f64; 3],
    extrusion_direction: [f64; 3],
    length: f64,
) -> Option<ExtrusionSpan> {
    (length.is_finite() && length > 0.0).then_some(())?;
    let profile_direction = normalized(profile_direction)?;
    let extrusion_direction = normalized(extrusion_direction)?;
    let alignment = dot(profile_direction, extrusion_direction);
    (alignment.abs() >= 1.0 - 1e-9).then_some(())?;
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

pub(super) fn feature_id_for_section_transform(
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

pub(super) fn derived_blind_extrusion_span(
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

pub(super) fn resolved_feature_extrusion_span(
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

pub(super) fn extruded_geometry_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<SurfaceGeometry> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let line = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            let normal = normalized(cross(line, transform.normal))?;
            Some(SurfaceGeometry::Plane {
                origin: Point3::new(start[0], start[1], start[2]),
                normal: Vector3::new(normal[0], normal[1], normal[2]),
                u_axis: Vector3::new(line[0], line[1], line[2]),
            })
        }
        SketchGeometry::Arc { center, radius, .. } | SketchGeometry::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius: radius.0,
            })
        }
        _ => None,
    }
}

pub(super) fn revolved_section_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    revolution_axis: RevolutionAxis,
) -> Option<SurfaceGeometry> {
    let axis = normalized([
        revolution_axis.direction.x,
        revolution_axis.direction.y,
        revolution_axis.direction.z,
    ])?;
    let axis_origin = [
        revolution_axis.origin.x,
        revolution_axis.origin.y,
        revolution_axis.origin.z,
    ];
    let project = |point: [f64; 3]| {
        let displacement = std::array::from_fn(|index| point[index] - axis_origin[index]);
        let axial = dot(displacement, axis);
        let on_axis = std::array::from_fn(|index| axis_origin[index] + axial * axis[index]);
        let radial = std::array::from_fn(|index| point[index] - on_axis[index]);
        (on_axis, radial)
    };
    let vector = |values: [f64; 3]| Vector3::new(values[0], values[1], values[2]);
    let point = |values: [f64; 3]| Point3::new(values[0], values[1], values[2]);
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalized(std::array::from_fn(|index| end[index] - start[index]))?;
            let (mut on_axis, mut radial) = project(start);
            let mut radius = dot(radial, radial).sqrt();
            if radius <= 1e-10 {
                (on_axis, radial) = project(end);
                radius = dot(radial, radial).sqrt();
            }
            let axial_rate = dot(direction, axis);
            let radial_rate =
                std::array::from_fn(|index| direction[index] - axial_rate * axis[index]);
            let radial_speed = dot(radial_rate, radial_rate).sqrt();
            let scale = radius.max(1.0);
            if radius > 1e-10 {
                let coplanar_residual = dot(cross(radial, radial_rate), axis).abs();
                (coplanar_residual <= 1e-9 * scale).then_some(())?;
            }
            let reference = normalized(radial).or_else(|| normalized(radial_rate))?;
            if radial_speed <= 1e-10 {
                (radius > 1e-10).then_some(())?;
                return Some(SurfaceGeometry::Cylinder {
                    origin: point(on_axis),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    radius,
                });
            }
            if axial_rate.abs() <= 1e-10 {
                return Some(SurfaceGeometry::Plane {
                    origin: point(on_axis),
                    normal: vector(axis),
                    u_axis: vector(reference),
                });
            }
            let radial_rate = dot(radial_rate, reference);
            let cone_axis = if radial_rate / axial_rate < 0.0 {
                std::array::from_fn(|index| -axis[index])
            } else {
                axis
            };
            Some(SurfaceGeometry::Cone {
                origin: point(on_axis),
                axis: vector(cone_axis),
                ref_direction: vector(reference),
                radius,
                ratio: 1.0,
                half_angle: radial_rate.abs().atan2(axial_rate.abs()),
            })
        }
        SketchGeometry::Arc { center, radius, .. } | SketchGeometry::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            let (on_axis, radial) = project(center);
            let major_radius = dot(radial, radial).sqrt();
            let reference = normalized(radial).or_else(|| {
                [transform.u_axis, transform.v_axis]
                    .into_iter()
                    .find_map(|candidate| {
                        let axial = dot(candidate, axis);
                        normalized(std::array::from_fn(|index| {
                            candidate[index] - axial * axis[index]
                        }))
                    })
            })?;
            if major_radius <= 1e-10 {
                Some(SurfaceGeometry::Sphere {
                    center: point(center),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    radius: radius.0,
                })
            } else {
                Some(SurfaceGeometry::Torus {
                    center: point(on_axis),
                    axis: vector(axis),
                    ref_direction: vector(reference),
                    major_radius,
                    minor_radius: radius.0,
                })
            }
        }
        _ => None,
    }
}

pub(super) fn placed_section_geometry_curve(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
) -> Option<CurveGeometry> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            let start = section_point_in_model(transform, [start.u, start.v]);
            let end = section_point_in_model(transform, [end.u, end.v]);
            let direction = normalized(std::array::from_fn(|axis| end[axis] - start[axis]))?;
            Some(CurveGeometry::Line {
                origin: Point3::new(start[0], start[1], start[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SketchGeometry::ReferenceLine { origin, direction } => {
            let origin = section_point_in_model(transform, [origin.u, origin.v]);
            let direction = normalized([
                direction.u * transform.u_axis[0] + direction.v * transform.v_axis[0],
                direction.u * transform.u_axis[1] + direction.v * transform.v_axis[1],
                direction.u * transform.u_axis[2] + direction.v * transform.v_axis[2],
            ])?;
            Some(CurveGeometry::Line {
                origin: Point3::new(origin[0], origin[1], origin[2]),
                direction: Vector3::new(direction[0], direction[1], direction[2]),
            })
        }
        SketchGeometry::Arc { center, radius, .. } | SketchGeometry::Circle { center, radius } => {
            let center = section_point_in_model(transform, [center.u, center.v]);
            Some(CurveGeometry::Circle {
                center: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius: radius.0,
            })
        }
        _ => None,
    }
}

pub(super) fn placed_sketch_curve_ref(
    transform: Option<&crate::placement::FeatureSectionTransform>,
    sketch: &SketchId,
    suffix: impl std::fmt::Display,
    geometry: &SketchGeometry,
) -> Option<String> {
    placed_section_geometry_curve(transform?, geometry)?;
    Some(sketch_section_curve_id(sketch, suffix))
}

pub(super) fn bspline_basis(
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> f64 {
    if parameter == *knots.last().expect("nonempty knots") {
        return if index + 1 == count { 1.0 } else { 0.0 };
    }
    if degree == 0 {
        return if knots[index] <= parameter && parameter < knots[index + 1] {
            1.0
        } else {
            0.0
        };
    }
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        (parameter - knots[index]) / left_denominator
            * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        (knots[index + degree + 1] - parameter) / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left + right
}

pub(super) fn bspline_basis_derivative(
    index: usize,
    degree: usize,
    parameter: f64,
    knots: &[f64],
    count: usize,
) -> f64 {
    let left_denominator = knots[index + degree] - knots[index];
    let right_denominator = knots[index + degree + 1] - knots[index + 1];
    let left = if left_denominator > 0.0 {
        degree as f64 / left_denominator * bspline_basis(index, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    let right = if right_denominator > 0.0 {
        degree as f64 / right_denominator
            * bspline_basis(index + 1, degree - 1, parameter, knots, count)
    } else {
        0.0
    };
    left - right
}

pub(super) fn solve_vector_system(
    mut matrix: Vec<Vec<f64>>,
    mut values: Vec<[f64; 3]>,
) -> Option<Vec<[f64; 3]>> {
    let count = matrix.len();
    (values.len() == count && matrix.iter().all(|row| row.len() == count)).then_some(())?;
    for column in 0..count {
        let pivot = (column..count).max_by(|left, right| {
            matrix[*left][column]
                .abs()
                .total_cmp(&matrix[*right][column].abs())
        })?;
        (matrix[pivot][column].abs() > 1e-14).then_some(())?;
        matrix.swap(column, pivot);
        values.swap(column, pivot);
        let scale = matrix[column][column];
        for value in &mut matrix[column][column..] {
            *value /= scale;
        }
        values[column] = values[column].map(|value| value / scale);
        let pivot_row = matrix[column].clone();
        let pivot_value = values[column];
        for row in 0..count {
            if row == column {
                continue;
            }
            let factor = matrix[row][column];
            if factor == 0.0 {
                continue;
            }
            for (entry, pivot_entry) in matrix[row][column..].iter_mut().zip(&pivot_row[column..]) {
                *entry -= factor * pivot_entry;
            }
            for (value, pivot) in values[row].iter_mut().zip(pivot_value) {
                *value -= factor * pivot;
            }
        }
    }
    Some(values)
}

pub(super) fn interpolation_curve_data(
    points: &[[f64; 3]],
    parameters: &[f64],
    endpoint_derivatives: [[f64; 3]; 2],
) -> Option<(Vec<f64>, Vec<[f64; 3]>)> {
    const DEGREE: usize = 3;
    let point_count = points.len();
    (point_count >= 2 && parameters.len() == point_count).then_some(())?;
    parameters
        .windows(2)
        .all(|pair| pair[0].is_finite() && pair[0] < pair[1])
        .then_some(())?;
    parameters.last()?.is_finite().then_some(())?;
    let control_count = point_count + 2;
    let mut knots = vec![parameters[0]; DEGREE + 1];
    knots.extend_from_slice(&parameters[1..point_count - 1]);
    knots.extend(std::iter::repeat_n(parameters[point_count - 1], DEGREE + 1));
    let mut matrix = Vec::with_capacity(control_count);
    for parameter in parameters {
        matrix.push(
            (0..control_count)
                .map(|index| bspline_basis(index, DEGREE, *parameter, &knots, control_count))
                .collect(),
        );
    }
    for parameter in [parameters[0], parameters[point_count - 1]] {
        matrix.push(
            (0..control_count)
                .map(|index| {
                    bspline_basis_derivative(index, DEGREE, parameter, &knots, control_count)
                })
                .collect(),
        );
    }
    let mut values = points.to_vec();
    values.extend(endpoint_derivatives);
    Some((knots, solve_vector_system(matrix, values)?))
}

pub(super) fn saved_spline_nurbs(
    spline: &crate::feature::FeatureSavedSpline,
) -> Option<NurbsCurve> {
    (usize::try_from(spline.declared_point_count?).ok()? == spline.interpolation_points.len())
        .then_some(())?;
    let parameters = spline.parameters.as_ref()?;
    let tangents = spline.endpoint_tangents?;
    let (knots, control_points) =
        interpolation_curve_data(&spline.interpolation_points, parameters, tangents)?;
    let control_points = control_points
        .into_iter()
        .map(|point| Point3::new(point[0], point[1], point[2]))
        .collect();
    Some(NurbsCurve {
        degree: 3,
        knots,
        control_points,
        weights: None,
        periodic: false,
    })
}

pub(super) fn saved_spline_sketch_geometry(
    spline: &crate::feature::FeatureSavedSpline,
) -> Option<SketchGeometry> {
    let nurbs = saved_spline_nurbs(spline)?;
    nurbs
        .control_points
        .iter()
        .all(|point| point.z.abs() <= 1e-12)
        .then(|| SketchGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots,
            control_points: nurbs
                .control_points
                .into_iter()
                .map(|point| cadmpeg_ir::math::Point2::new(point.x, point.y))
                .collect(),
            weights: nurbs.weights,
            periodic: nurbs.periodic,
        })
}

pub(super) fn interpolation_spline_surface(
    points: &[[f64; 3]],
    u_parameters: &[f64],
    v_parameters: &[f64],
    end_u_derivatives: &[[f64; 3]],
    end_v_derivatives: &[[f64; 3]],
    corner_mixed_derivatives: &[[f64; 3]],
) -> Option<NurbsSurface> {
    let u_sample_count = u_parameters.len();
    let v_sample_count = v_parameters.len();
    let point_count = u_sample_count.checked_mul(v_sample_count)?;
    let u_boundary_derivative_count = v_sample_count.checked_mul(2)?;
    let v_boundary_derivative_count = u_sample_count.checked_mul(2)?;
    (points.len() == point_count
        && end_u_derivatives.len() == u_boundary_derivative_count
        && end_v_derivatives.len() == v_boundary_derivative_count
        && corner_mixed_derivatives.len() == 4)
        .then_some(())?;

    let u_control_count = u_sample_count.checked_add(2)?;
    let v_control_count = v_sample_count.checked_add(2)?;
    let mut position_controls = vec![vec![[0.0; 3]; v_sample_count]; u_control_count];
    let mut u_knots = None;
    for v in 0..v_sample_count {
        let samples = (0..u_sample_count)
            .map(|u| points[u * v_sample_count + v])
            .collect::<Vec<_>>();
        let (knots, controls) = interpolation_curve_data(
            &samples,
            u_parameters,
            [end_u_derivatives[v], end_u_derivatives[v_sample_count + v]],
        )?;
        u_knots.get_or_insert(knots);
        for (u, control) in controls.into_iter().enumerate() {
            position_controls[u][v] = control;
        }
    }

    let mut v_derivative_controls = vec![vec![[0.0; 3]; u_control_count]; 2];
    for v_boundary in 0..2 {
        let samples = (0..u_sample_count)
            .map(|u| end_v_derivatives[v_boundary * u_sample_count + u])
            .collect::<Vec<_>>();
        let (_, controls) = interpolation_curve_data(
            &samples,
            u_parameters,
            [
                corner_mixed_derivatives[v_boundary * 2],
                corner_mixed_derivatives[v_boundary * 2 + 1],
            ],
        )?;
        v_derivative_controls[v_boundary] = controls;
    }

    let mut control_points = Vec::with_capacity(u_control_count * v_control_count);
    let mut v_knots = None;
    for u in 0..u_control_count {
        let (knots, controls) = interpolation_curve_data(
            &position_controls[u],
            v_parameters,
            [v_derivative_controls[0][u], v_derivative_controls[1][u]],
        )?;
        v_knots.get_or_insert(knots);
        control_points.extend(
            controls
                .into_iter()
                .map(|point| Point3::new(point[0], point[1], point[2])),
        );
    }

    Some(NurbsSurface {
        u_degree: 3,
        v_degree: 3,
        u_knots: u_knots?,
        v_knots: v_knots?,
        u_count: u32::try_from(u_control_count).ok()?,
        v_count: u32::try_from(v_control_count).ok()?,
        control_points,
        weights: None,
        u_periodic: false,
        v_periodic: false,
    })
}

pub(super) fn placed_section_nurbs(
    transform: &crate::placement::FeatureSectionTransform,
    nurbs: &NurbsCurve,
) -> NurbsCurve {
    NurbsCurve {
        degree: nurbs.degree,
        knots: nurbs.knots.clone(),
        control_points: nurbs
            .control_points
            .iter()
            .map(|point| {
                let placed = section_xyz_in_model(transform, [point.x, point.y, point.z]);
                Point3::new(placed[0], placed[1], placed[2])
            })
            .collect(),
        weights: nurbs.weights.clone(),
        periodic: nurbs.periodic,
    }
}

pub(super) fn translated_nurbs_curve(curve: &NurbsCurve, translation: [f64; 3]) -> NurbsCurve {
    NurbsCurve {
        degree: curve.degree,
        knots: curve.knots.clone(),
        control_points: curve
            .control_points
            .iter()
            .map(|point| {
                Point3::new(
                    point.x + translation[0],
                    point.y + translation[1],
                    point.z + translation[2],
                )
            })
            .collect(),
        weights: curve.weights.clone(),
        periodic: curve.periodic,
    }
}

pub(super) fn extruded_nurbs_surface(
    directrix: &NurbsCurve,
    sweep: [f64; 3],
) -> Option<NurbsSurface> {
    if directrix
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != directrix.control_points.len())
    {
        return None;
    }
    let mut control_points = Vec::with_capacity(directrix.control_points.len() * 2);
    let mut weights = directrix
        .weights
        .as_ref()
        .map(|_| Vec::with_capacity(control_points.capacity()));
    for (index, point) in directrix.control_points.iter().enumerate() {
        control_points.push(*point);
        control_points.push(Point3::new(
            point.x + sweep[0],
            point.y + sweep[1],
            point.z + sweep[2],
        ));
        if let (Some(source), Some(target)) = (&directrix.weights, &mut weights) {
            target.extend([source[index], source[index]]);
        }
    }
    Some(NurbsSurface {
        u_degree: directrix.degree,
        v_degree: 1,
        u_knots: directrix.knots.clone(),
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: u32::try_from(directrix.control_points.len()).ok()?,
        v_count: 2,
        control_points,
        weights,
        u_periodic: directrix.periodic,
        v_periodic: false,
    })
}

pub(super) fn sketch_nurbs_curve(geometry: &SketchGeometry) -> Option<NurbsCurve> {
    let SketchGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = geometry
    else {
        return None;
    };
    let nurbs = NurbsCurve {
        degree: *degree,
        knots: knots.clone(),
        control_points: control_points
            .iter()
            .map(|point| Point3::new(point.u, point.v, 0.0))
            .collect(),
        weights: weights.clone(),
        periodic: *periodic,
    };
    valid_positive_nurbs_curve(&nurbs).map(|()| nurbs)
}

pub(super) fn oriented_sketch_nurbs_curve(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<NurbsCurve> {
    let nurbs = sketch_nurbs_curve(geometry)?;
    if !reversed {
        return Some(nurbs);
    }
    let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
    Some(NurbsCurve {
        degree: nurbs.degree,
        knots: nurbs
            .knots
            .iter()
            .rev()
            .map(|knot| lower + upper - knot)
            .collect(),
        control_points: nurbs.control_points.into_iter().rev().collect(),
        weights: nurbs
            .weights
            .map(|weights| weights.into_iter().rev().collect()),
        periodic: nurbs.periodic,
    })
}

pub(super) fn sketch_nurbs_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<PcurveGeometry> {
    let nurbs = oriented_sketch_nurbs_curve(geometry, reversed)?;
    Some(PcurveGeometry::Nurbs {
        degree: nurbs.degree,
        knots: nurbs.knots,
        control_points: nurbs
            .control_points
            .into_iter()
            .map(|point| Point2::new(point.x, point.y))
            .collect(),
        weights: nurbs.weights,
        periodic: nurbs.periodic,
    })
}

pub(super) fn extrusion_brep_side_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
) -> Option<SurfaceGeometry> {
    if matches!(geometry, SketchGeometry::Nurbs { .. }) {
        let directrix = oriented_sketch_nurbs_curve(geometry, reversed)?;
        let placed = placed_section_nurbs(transform, &directrix);
        let lower_translation = transform.normal.map(|value| value * span.lower);
        let sweep = transform
            .normal
            .map(|value| value * (span.upper - span.lower));
        return Some(SurfaceGeometry::Nurbs(extruded_nurbs_surface(
            &translated_nurbs_curve(&placed, lower_translation),
            sweep,
        )?));
    }
    let section_geometry = match geometry {
        SketchGeometry::Line { .. } => SketchGeometry::Line {
            start: Point2::new(start[0], start[1]),
            end: Point2::new(end[0], end[1]),
        },
        value => value.clone(),
    };
    extruded_geometry_surface(transform, &section_geometry)
}

pub(super) fn signed_unit_chart(
    local: [f64; 2],
    frame: [f64; 2],
    offset: f64,
) -> Option<(f64, f64)> {
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
    };
    let mut matches = Vec::new();
    for first_sign in [-1.0, 1.0] {
        for second_sign in [-1.0, 1.0] {
            let frame = [first_sign * frame[0], second_sign * frame[1]];
            for reversed in [false, true] {
                let target = if reversed {
                    [frame[1], frame[0]]
                } else {
                    frame
                };
                let slope = if reversed { -1.0 } else { 1.0 };
                let chart_intercept = target[0] - slope * local[0];
                if close(target[1], slope * local[1] + chart_intercept)
                    && close(chart_intercept.abs(), offset)
                    && !matches.contains(&(slope, chart_intercept))
                {
                    matches.push((slope, chart_intercept));
                }
            }
        }
    }
    let [mapping] = matches.as_slice() else {
        return None;
    };
    Some(*mapping)
}

pub(super) fn placed_tabulated_cylinder_directrix(
    replay: &crate::surface::TabulatedCylinderCurveReplay,
    parameters: &crate::surface::SurfaceParameterRecord,
    chart_origin: Option<[f64; 3]>,
) -> Option<(NurbsCurve, [f64; 3])> {
    #[derive(Clone, Copy)]
    enum FrameLayout {
        LegacyReflected,
        PrototypeOffsetPlanar,
        ZeroOffsetPlanar,
        SelectedPlanar,
    }
    if parameters.boundary != crate::surface::SurfaceBodyBoundary::CompoundClose {
        return None;
    }
    let points = replay
        .control_points
        .iter()
        .copied()
        .collect::<Option<Vec<_>>>()?;
    let (values, layout) = parameters
        .tabulated_cylinder_frame
        .map(|frame| {
            let values = frame.values.to_vec();
            let heads = frame.prefixes;
            let offset_planar_layout = matches!(heads.as_slice(), [_, 0x46, _, _, 0x46, _]);
            let zero_offset_layout = matches!(heads.as_slice(), [_, 0x42, _, _, 0x18, _]);
            if offset_planar_layout {
                (values, FrameLayout::PrototypeOffsetPlanar)
            } else if zero_offset_layout {
                (values, FrameLayout::ZeroOffsetPlanar)
            } else {
                (values, FrameLayout::SelectedPlanar)
            }
        })
        .or_else(|| {
            let [_, frame] = parameters.scalar_frames.as_slice() else {
                return None;
            };
            let values = frame
                .slots
                .iter()
                .map(|slot| slot.value)
                .collect::<Option<Vec<_>>>()?;
            Some((values, FrameLayout::LegacyReflected))
        })?;
    let [a0, a1, a2, b0, b1, b2] = values.as_slice() else {
        return None;
    };
    let first = [*a0, *a1, *a2];
    let second = [*b0, *b1, *b2];
    let local_start = points.first()?;
    let local_end = points.last()?;
    let local_span = [
        (local_end[0] - local_start[0]).abs(),
        (local_end[1] - local_start[1]).abs(),
    ];
    if local_span
        .iter()
        .any(|span| !span.is_finite() || *span <= 0.0)
    {
        return None;
    }
    let close = |left: f64, right: f64| {
        (left - right).abs() <= 1.0e-9 * left.abs().max(right.abs()).max(1.0)
    };
    let axis_matches = |axis: usize, coordinate: usize| match layout {
        FrameLayout::LegacyReflected => {
            close((second[axis] - first[axis]).abs(), local_span[coordinate])
        }
        FrameLayout::PrototypeOffsetPlanar => chart_origin.is_some_and(|origin| {
            signed_unit_chart(
                [local_start[coordinate], local_end[coordinate]],
                [first[axis], second[axis]],
                if coordinate == 0 {
                    origin[axis].abs()
                } else {
                    0.0
                },
            )
            .is_some()
        }),
        FrameLayout::ZeroOffsetPlanar => signed_unit_chart(
            [local_start[coordinate], local_end[coordinate]],
            [first[axis], second[axis]],
            0.0,
        )
        .is_some(),
        FrameLayout::SelectedPlanar => {
            let zero_offset = signed_unit_chart(
                [local_start[coordinate], local_end[coordinate]],
                [first[axis], second[axis]],
                0.0,
            )
            .is_some();
            let prototype_offset = (coordinate == 0)
                .then(|| chart_origin.map(|origin| origin[axis].abs()))
                .flatten()
                .filter(|offset| offset.is_finite() && !close(*offset, 0.0))
                .is_some_and(|offset| {
                    signed_unit_chart(
                        [local_start[coordinate], local_end[coordinate]],
                        [first[axis], second[axis]],
                        offset,
                    )
                    .is_some()
                });
            zero_offset || prototype_offset
        }
    };
    let assignments = (0..3)
        .flat_map(|first_axis| {
            (0..3)
                .filter(move |&second_axis| {
                    first_axis != second_axis
                        && axis_matches(first_axis, 0)
                        && axis_matches(second_axis, 1)
                })
                .map(move |second_axis| (first_axis, second_axis, 3 - first_axis - second_axis))
        })
        .collect::<Vec<_>>();
    let [(first_axis, second_axis, sweep_axis)] = assignments.as_slice() else {
        return None;
    };
    let (signed_chart, reflect_sweep) = match layout {
        FrameLayout::LegacyReflected => (None, false),
        FrameLayout::PrototypeOffsetPlanar => (
            Some((
                signed_unit_chart(
                    [local_start[0], local_end[0]],
                    [first[*first_axis], second[*first_axis]],
                    chart_origin?[*first_axis].abs(),
                )?,
                signed_unit_chart(
                    [local_start[1], local_end[1]],
                    [first[*second_axis], second[*second_axis]],
                    0.0,
                )?,
            )),
            false,
        ),
        FrameLayout::ZeroOffsetPlanar => (
            Some((
                signed_unit_chart(
                    [local_start[0], local_end[0]],
                    [first[*first_axis], second[*first_axis]],
                    0.0,
                )?,
                signed_unit_chart(
                    [local_start[1], local_end[1]],
                    [first[*second_axis], second[*second_axis]],
                    0.0,
                )?,
            )),
            false,
        ),
        FrameLayout::SelectedPlanar => {
            let mut first_intercepts = vec![(0.0, false)];
            if let Some(origin) = chart_origin {
                let intercept = origin[*first_axis].abs();
                if intercept.is_finite() && !close(intercept, 0.0) {
                    first_intercepts.push((intercept, true));
                }
            }
            let candidates = first_intercepts
                .into_iter()
                .filter_map(|(first_offset, reflect_sweep)| {
                    Some((
                        (
                            signed_unit_chart(
                                [local_start[0], local_end[0]],
                                [first[*first_axis], second[*first_axis]],
                                first_offset,
                            )?,
                            signed_unit_chart(
                                [local_start[1], local_end[1]],
                                [first[*second_axis], second[*second_axis]],
                                0.0,
                            )?,
                        ),
                        reflect_sweep,
                    ))
                })
                .collect::<Vec<_>>();
            let [(chart, reflect_sweep)] = candidates.as_slice() else {
                return None;
            };
            (Some(*chart), *reflect_sweep)
        }
    };
    let control_points = points
        .iter()
        .map(|point| {
            let mut placed = [0.0; 3];
            match signed_chart {
                Some(((first_slope, first_intercept), (second_slope, second_intercept))) => {
                    placed[*first_axis] = first_slope * point[0] + first_intercept;
                    placed[*second_axis] = second_slope * point[1] + second_intercept;
                    placed[*sweep_axis] = if reflect_sweep {
                        -first[*sweep_axis]
                    } else {
                        first[*sweep_axis]
                    };
                }
                None => {
                    let chart_first =
                        first[*first_axis].max(second[*first_axis]) - (point[0] - local_start[0]);
                    let chart_second =
                        first[*second_axis].min(second[*second_axis]) + (point[1] - local_start[1]);
                    placed[*first_axis] = if *first_axis < 2 {
                        -chart_first
                    } else {
                        chart_first
                    };
                    placed[*second_axis] = if *second_axis < 2 {
                        -chart_second
                    } else {
                        chart_second
                    };
                    placed[*sweep_axis] = first[*sweep_axis];
                }
            }
            Point3::new(placed[0], placed[1], placed[2])
        })
        .collect();
    let mut sweep = [0.0; 3];
    sweep[*sweep_axis] = if reflect_sweep {
        first[*sweep_axis] - second[*sweep_axis]
    } else {
        second[*sweep_axis] - first[*sweep_axis]
    };
    (sweep[*sweep_axis].is_finite() && sweep[*sweep_axis] != 0.0).then_some((
        NurbsCurve {
            degree: 3,
            knots: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
            control_points,
            weights: None,
            periodic: false,
        },
        sweep,
    ))
}

pub(super) fn transfer_saved_spline_curves(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        for spline in
            semantic_saved_section_entities(definition).filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
        {
            let Some(nurbs) = saved_spline_nurbs(spline) else {
                continue;
            };
            let suffix = spline.entity_id.map_or_else(
                || format!("offset{}", spline.offset),
                |entity_id| entity_id.to_string(),
            );
            let curve_id = CurveId(format!(
                "creo:featdefs:saved_spline_curve#{}:{suffix}",
                definition.id
            ));
            if ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                continue;
            }
            annotate(
                annotations,
                &curve_id,
                "FeatDefs",
                spline.offset as u64,
                "placed_saved_interpolation_spline",
                Exactness::Derived,
            );
            ir.model.curves.push(Curve {
                id: curve_id,
                geometry: CurveGeometry::Nurbs(placed_section_nurbs(transform, &nurbs)),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("FeatDefs:saved_spline#{suffix}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }
    }
    transferred
}

pub(super) fn revolved_nurbs_surface(
    directrix: &NurbsCurve,
    axis: RevolutionAxis,
) -> Option<NurbsSurface> {
    if directrix
        .weights
        .as_ref()
        .is_some_and(|weights| weights.len() != directrix.control_points.len())
    {
        return None;
    }
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let angular_poles = [
        [1.0, 0.0],
        [1.0, 1.0],
        [0.0, 1.0],
        [-1.0, 1.0],
        [-1.0, 0.0],
        [-1.0, -1.0],
        [0.0, -1.0],
        [1.0, -1.0],
        [1.0, 0.0],
    ];
    let diagonal_weight = std::f64::consts::FRAC_1_SQRT_2;
    let angular_weights = [
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
        diagonal_weight,
        1.0,
    ];
    let mut control_points = Vec::with_capacity(directrix.control_points.len() * 9);
    let mut weights = Vec::with_capacity(directrix.control_points.len() * 9);
    for (index, point) in directrix.control_points.iter().enumerate() {
        let relative = [
            point.x - axis_origin[0],
            point.y - axis_origin[1],
            point.z - axis_origin[2],
        ];
        let axial_distance = dot(relative, axis_direction);
        let center: [f64; 3] = std::array::from_fn(|component| {
            axis_origin[component] + axial_distance * axis_direction[component]
        });
        let radial = [
            point.x - center[0],
            point.y - center[1],
            point.z - center[2],
        ];
        let tangent = cross(axis_direction, radial);
        let directrix_weight = directrix
            .weights
            .as_ref()
            .map_or(1.0, |curve_weights| curve_weights[index]);
        for ([radial_scale, tangent_scale], angular_weight) in
            angular_poles.into_iter().zip(angular_weights)
        {
            control_points.push(Point3::new(
                center[0] + radial_scale * radial[0] + tangent_scale * tangent[0],
                center[1] + radial_scale * radial[1] + tangent_scale * tangent[1],
                center[2] + radial_scale * radial[2] + tangent_scale * tangent[2],
            ));
            weights.push(directrix_weight * angular_weight);
        }
    }
    Some(NurbsSurface {
        u_degree: directrix.degree,
        v_degree: 2,
        u_knots: directrix.knots.clone(),
        v_knots: vec![
            0.0,
            0.0,
            0.0,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::FRAC_PI_2,
            std::f64::consts::PI,
            std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2,
            3.0 * std::f64::consts::FRAC_PI_2,
            std::f64::consts::TAU,
            std::f64::consts::TAU,
            std::f64::consts::TAU,
        ],
        u_count: u32::try_from(directrix.control_points.len()).ok()?,
        v_count: 9,
        control_points,
        weights: Some(weights),
        u_periodic: false,
        v_periodic: false,
    })
}

pub(super) fn revolved_section_circle(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
    axis: RevolutionAxis,
) -> Option<CurveGeometry> {
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let point = section_point_in_model(transform, point);
    let relative: [f64; 3] =
        std::array::from_fn(|component| point[component] - axis_origin[component]);
    let axial_distance = dot(relative, axis_direction);
    let center: [f64; 3] = std::array::from_fn(|component| {
        axis_origin[component] + axial_distance * axis_direction[component]
    });
    let radial: [f64; 3] = std::array::from_fn(|component| point[component] - center[component]);
    let radius = dot(radial, radial).sqrt();
    let scale = point
        .iter()
        .chain(&axis_origin)
        .map(|coordinate| coordinate.abs())
        .fold(1.0, f64::max);
    (radius > 1e-10 * scale).then_some(())?;
    let reference = radial.map(|component| component / radius);
    Some(CurveGeometry::Circle {
        center: Point3::new(center[0], center[1], center[2]),
        axis: Vector3::new(axis_direction[0], axis_direction[1], axis_direction[2]),
        ref_direction: Vector3::new(reference[0], reference[1], reference[2]),
        radius,
    })
}

pub(super) fn extruded_section_line(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> Option<CurveGeometry> {
    let direction = normalized(transform.normal)?;
    let origin = section_point_in_model(transform, point);
    Some(CurveGeometry::Line {
        origin: Point3::new(origin[0], origin[1], origin[2]),
        direction: Vector3::new(direction[0], direction[1], direction[2]),
    })
}

pub(super) fn transfer_feature_extrusion_surfaces(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_linear_extrusion(scan, feature_id) {
            continue;
        }
        let Some(order_table) = &definition.order_table else {
            continue;
        };
        let points = resolved_section_points(definition);
        let solved = definition
            .trim_entities
            .iter()
            .flat_map(|trim_entities| &trim_entities.rows)
            .filter_map(|row| trim_segment_id(definition, row))
            .collect::<BTreeSet<_>>();
        for segment in complete_section_segment_rows(definition)
            .iter()
            .filter(|segment| solved.contains(&segment.external_id))
        {
            let Some(section_geometry) =
                resolved_section_segment_geometry(definition, &points, segment)
            else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(surface_id) = analytic_surface_id_for_feature(
                &scan.surfaces.rows,
                &scan.features.entity_tables,
                feature_id,
                segment.external_id,
                &geometry,
            ) else {
                continue;
            };
            let id = SurfaceId(format!("creo:visibgeom:surface#{surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                segment.offset as u64,
                "protextrude_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }

        for (internal_id, section_geometry, offset) in
            semantic_saved_section_entities(definition).filter_map(saved_section_entity_geometry)
        {
            let Some(external_id) = order_table.external_id(internal_id) else {
                continue;
            };
            let Some(native_surface_id) = generated_surface_id_for_feature(
                &scan.features.entity_tables,
                feature_id,
                external_id,
            ) else {
                continue;
            };
            let Some(geometry) = extruded_geometry_surface(transform, &section_geometry) else {
                continue;
            };
            let Some(expected_kind) = surface_kind_for_geometry(&geometry) else {
                continue;
            };
            if !scan.surfaces.rows.iter().any(|row| {
                row.id == native_surface_id
                    && row.feature_id == feature_id
                    && row.kind == expected_kind
            }) {
                continue;
            }
            let id = SurfaceId(format!("creo:visibgeom:surface#{native_surface_id}"));
            if ir.model.surfaces.iter().any(|surface| surface.id == id) {
                continue;
            }
            annotate(
                annotations,
                &id,
                "FeatDefs",
                offset as u64,
                "protextrude_saved_section_carrier",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id,
                geometry,
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            transferred += 1;
        }

        let splines = semantic_saved_section_entities(definition)
            .filter_map(|entity| match entity {
                crate::feature::FeatureSavedEntity::Spline(spline) => Some(spline),
                _ => None,
            })
            .filter_map(|spline| {
                let internal_id = spline.entity_id?;
                let external_id = order_table.external_id(internal_id)?;
                let surface_id = generated_surface_id_for_feature(
                    &scan.features.entity_tables,
                    feature_id,
                    external_id,
                )?;
                scan.surfaces
                    .rows
                    .iter()
                    .any(|row| {
                        row.id == surface_id
                            && row.feature_id == feature_id
                            && row.kind == crate::surface::SurfaceKind::Extrusion
                    })
                    .then_some((surface_id, spline))
            })
            .collect::<Vec<_>>();
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let lower_translation = transform.normal.map(|value| value * span.lower);
        let sweep = transform
            .normal
            .map(|value| value * (span.upper - span.lower));
        for (native_surface_id, spline) in splines {
            let Some(section_curve) = saved_spline_nurbs(spline) else {
                continue;
            };
            let placed = placed_section_nurbs(transform, &section_curve);
            let directrix = translated_nurbs_curve(&placed, lower_translation);
            let Some(surface) = extruded_nurbs_surface(&directrix, sweep) else {
                continue;
            };
            let suffix = spline
                .entity_id
                .expect("ordered saved spline has an entity id")
                .to_string();
            let curve_id = CurveId(format!(
                "creo:feature:extrusion_directrix#{feature_id}:{suffix}"
            ));
            if !ir.model.curves.iter().any(|curve| curve.id == curve_id) {
                annotate(
                    annotations,
                    &curve_id,
                    "FeatDefs",
                    spline.offset as u64,
                    "protextrude_spline_directrix",
                    Exactness::Derived,
                );
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Nurbs(directrix.clone()),
                    source_object: Some(SourceObjectAssociation {
                        format: "creo".to_string(),
                        object_id: format!("FeatDefs:saved_spline#{suffix}"),
                        name: None,
                        color: None,
                        visible: None,
                        layer: None,
                        instance_path: Vec::new(),
                    }),
                });
            }
            let surface_id = SurfaceId(format!("creo:visibgeom:surface#{native_surface_id}"));
            if ir.model.surfaces.iter().any(|item| item.id == surface_id) {
                continue;
            }
            let procedural_id = ProceduralSurfaceId(format!(
                "creo:feature:extrusion_construction#{feature_id}:{suffix}"
            ));
            annotate(
                annotations,
                &surface_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface",
                Exactness::Derived,
            );
            annotate(
                annotations,
                &procedural_id,
                "FeatDefs",
                spline.offset as u64,
                "protextrude_spline_surface_construction",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: SurfaceGeometry::Nurbs(surface),
                source_object: Some(SourceObjectAssociation {
                    format: "creo".to_string(),
                    object_id: format!("VisibGeom:{native_surface_id}"),
                    name: None,
                    color: None,
                    visible: None,
                    layer: None,
                    instance_path: Vec::new(),
                }),
            });
            ir.model.procedural_surfaces.push(ProceduralSurface {
                id: procedural_id,
                surface: surface_id,
                definition: ProceduralSurfaceDefinition::Extrusion {
                    directrix: curve_id,
                    parameter_interval: Some([
                        *directrix.knots.first().expect("validated spline knots"),
                        *directrix.knots.last().expect("validated spline knots"),
                    ]),
                    direction: Vector3::new(sweep[0], sweep[1], sweep[2]),
                    native_position: None,
                    revision_form: None,
                },
                cache_fit_tolerance: None,
                record_bounds: None,
            });
            transferred += 1;
        }
    }
    transferred
}

pub(super) fn sketch_geometry_endpoints(geometry: &SketchGeometry) -> Option<([f64; 2], [f64; 2])> {
    match geometry {
        SketchGeometry::Line { start, end } => Some(([start.u, start.v], [end.u, end.v])),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some((
            [
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ],
            [
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ],
        )),
        SketchGeometry::Circle { center, radius }
            if center.u.is_finite()
                && center.v.is_finite()
                && radius.0.is_finite()
                && radius.0 > 0.0 =>
        {
            let seam = [center.u + radius.0, center.v];
            Some((seam, seam))
        }
        SketchGeometry::Nurbs { .. } => {
            let nurbs = sketch_nurbs_curve(geometry)?;
            let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
            let carrier = CurveGeometry::Nurbs(nurbs);
            let first = cadmpeg_ir::eval::curve_point(&carrier, lower)?;
            let last = cadmpeg_ir::eval::curve_point(&carrier, upper)?;
            [first.x, first.y]
                .into_iter()
                .chain([last.x, last.y])
                .all(f64::is_finite)
                .then_some(([first.x, first.y], [last.x, last.y]))
        }
        _ => None,
    }
}

pub(super) fn connected_sketch_profile_vertices(
    ir: &CadIr,
    sketch_id: &SketchId,
) -> Vec<(usize, Vec<[f64; 2]>)> {
    let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == *sketch_id)
    else {
        return Vec::new();
    };
    let entities = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch_id)
        .map(|entity| (entity.id.clone(), &entity.geometry))
        .collect::<BTreeMap<_, _>>();
    sketch
        .profiles
        .iter()
        .enumerate()
        .filter_map(|(profile_index, profile)| {
            (!profile.is_empty()).then_some(())?;
            let uses = profile
                .iter()
                .map(|entity_use| {
                    let geometry = entities.get(&entity_use.entity)?;
                    let (mut start, mut end) = sketch_geometry_endpoints(geometry)?;
                    if entity_use.reversed {
                        std::mem::swap(&mut start, &mut end);
                    }
                    Some((start, end))
                })
                .collect::<Option<Vec<_>>>()?;
            let scale = uses
                .iter()
                .flat_map(|(start, end)| start.iter().chain(end))
                .map(|coordinate| coordinate.abs())
                .fold(1.0, f64::max);
            uses.windows(2)
                .all(|adjacent| {
                    let end = adjacent[0].1;
                    let next = adjacent[1].0;
                    (end[0] - next[0]).hypot(end[1] - next[1]) <= 1e-9 * scale
                })
                .then(|| {
                    let mut vertices = uses.iter().map(|(start, _)| *start).collect::<Vec<_>>();
                    let first = uses[0].0;
                    let terminal = uses.last().expect("profile is not empty").1;
                    if (terminal[0] - first[0]).hypot(terminal[1] - first[1]) > 1e-9 * scale {
                        vertices.push(terminal);
                    }
                    (profile_index, vertices)
                })
        })
        .collect()
}

pub(super) fn oriented_arc_parameterization(
    reversed: bool,
    start: f64,
    end: f64,
) -> (f64, [f64; 2]) {
    let (axis_sign, raw_start, raw_end) = if reversed {
        (-1.0, -end, -start)
    } else {
        (1.0, start, end)
    };
    let raw_span = raw_end - raw_start;
    let full_turn = raw_span.is_finite()
        && (raw_span.abs() - std::f64::consts::TAU).abs()
            <= 1e-12 * raw_span.abs().max(std::f64::consts::TAU);
    let start = raw_start.rem_euclid(std::f64::consts::TAU);
    let mut end = raw_end.rem_euclid(std::f64::consts::TAU);
    if end < start || (full_turn && (end - start).abs() <= 1e-12) {
        end += std::f64::consts::TAU;
    }
    (axis_sign, [start, end])
}

pub(super) fn forward_arc_sweep(start: f64, end: f64) -> f64 {
    let raw_span = end - start;
    if raw_span.is_finite()
        && (raw_span - std::f64::consts::TAU).abs()
            <= 1e-12 * raw_span.abs().max(std::f64::consts::TAU)
    {
        std::f64::consts::TAU
    } else {
        raw_span.rem_euclid(std::f64::consts::TAU)
    }
}

pub(super) fn line_pcurve(start: [f64; 2], end: [f64; 2]) -> PcurveGeometry {
    PcurveGeometry::Line {
        origin: Point2::new(start[0], start[1]),
        direction: Point2::new(end[0] - start[0], end[1] - start[1]),
    }
}

pub(super) fn circular_pcurve(
    center: [f64; 2],
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> PcurveGeometry {
    let segment_count = ((end_angle - start_angle).abs() / std::f64::consts::FRAC_PI_2)
        .ceil()
        .max(1.0) as usize;
    let step = (end_angle - start_angle) / segment_count as f64;
    let mut control_points = Vec::with_capacity(2 * segment_count + 1);
    let mut weights = Vec::with_capacity(2 * segment_count + 1);
    for segment in 0..segment_count {
        let first = start_angle + segment as f64 * step;
        let second = first + step;
        let middle = 0.5 * (first + second);
        let middle_weight = (0.5 * step).cos();
        if segment == 0 {
            control_points.push(Point2::new(
                center[0] + radius * first.cos(),
                center[1] + radius * first.sin(),
            ));
            weights.push(1.0);
        }
        control_points.push(Point2::new(
            center[0] + radius * middle.cos() / middle_weight,
            center[1] + radius * middle.sin() / middle_weight,
        ));
        weights.push(middle_weight);
        control_points.push(Point2::new(
            center[0] + radius * second.cos(),
            center[1] + radius * second.sin(),
        ));
        weights.push(1.0);
    }
    let mut knots = vec![0.0; 3];
    for boundary in 1..segment_count {
        knots.extend([boundary as f64 / segment_count as f64; 2]);
    }
    knots.extend([1.0; 3]);
    PcurveGeometry::Nurbs {
        degree: 2,
        knots,
        control_points,
        weights: Some(weights),
        periodic: false,
    }
}

pub(super) fn extrusion_cap_pcurve(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
) -> PcurveGeometry {
    match geometry {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let [start_angle, end_angle] = if reversed {
                [end_angle.0, start_angle.0]
            } else {
                [start_angle.0, end_angle.0]
            };
            circular_pcurve([center.u, center.v], radius.0, start_angle, end_angle)
        }
        SketchGeometry::Circle { center, radius } => {
            let [start_angle, end_angle] = oriented_full_turn_angles(reversed);
            circular_pcurve([center.u, center.v], radius.0, start_angle, end_angle)
        }
        SketchGeometry::Nurbs { .. } => {
            sketch_nurbs_pcurve(geometry, reversed).unwrap_or_else(|| line_pcurve(start, end))
        }
        _ => line_pcurve(start, end),
    }
}

pub(super) fn extrusion_side_uvs(
    geometry: &SketchGeometry,
    reversed: bool,
    start: [f64; 2],
    end: [f64; 2],
    span: ExtrusionSpan,
) -> [[[f64; 2]; 2]; 4] {
    if matches!(geometry, SketchGeometry::Nurbs { .. }) {
        if let Some(nurbs) = oriented_sketch_nurbs_curve(geometry, reversed) {
            if let Some([lower, upper]) = nurbs_intrinsic_parameter_range(&nurbs) {
                return [
                    [[lower, 0.0], [upper, 0.0]],
                    [[upper, 0.0], [upper, 1.0]],
                    [[lower, 1.0], [upper, 1.0]],
                    [[lower, 0.0], [lower, 1.0]],
                ];
            }
        }
    }
    let [first, second] = match geometry {
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } if reversed => [end_angle.0, start_angle.0],
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } => [start_angle.0, end_angle.0],
        SketchGeometry::Circle { .. } => oriented_full_turn_angles(reversed),
        _ => [0.0, (end[0] - start[0]).hypot(end[1] - start[1])],
    };
    [
        [[first, span.lower], [second, span.lower]],
        [[second, span.lower], [second, span.upper]],
        [[first, span.upper], [second, span.upper]],
        [[first, span.lower], [first, span.upper]],
    ]
}

pub(super) fn extrusion_profile_signed_area(
    profile: &[(SketchGeometry, bool, [f64; 2], [f64; 2])],
) -> Option<f64> {
    let mut area_twice = 0.0;
    for (geometry, reversed, start, end) in profile {
        let contribution = match geometry {
            SketchGeometry::Nurbs { .. } => nurbs_profile_signed_area_twice(geometry, *reversed)?,
            SketchGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let forward_sweep = forward_arc_sweep(start_angle.0, end_angle.0);
                let sweep = if *reversed {
                    -forward_sweep
                } else {
                    forward_sweep
                };
                center.u.mul_add(
                    end[1] - start[1],
                    -(center.v * (end[0] - start[0])) + radius.0 * radius.0 * sweep,
                )
            }
            SketchGeometry::Circle { center, radius } => {
                let sweep = if *reversed {
                    -std::f64::consts::TAU
                } else {
                    std::f64::consts::TAU
                };
                center.u.mul_add(
                    end[1] - start[1],
                    -(center.v * (end[0] - start[0])) + radius.0 * radius.0 * sweep,
                )
            }
            _ => start[0].mul_add(end[1], -(start[1] * end[0])),
        };
        area_twice += contribution;
    }
    let scale = profile
        .iter()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    (area_twice.abs() > 1e-12 * scale * scale).then_some(0.5 * area_twice)
}

pub(super) type ExtrusionProfile = Vec<(SketchGeometry, bool, [f64; 2], [f64; 2])>;

pub(super) fn resolved_sketch_profiles(
    ir: &CadIr,
    sketch_id: &SketchId,
    minimum_entity_count: usize,
) -> Option<Vec<ExtrusionProfile>> {
    let sketch = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == *sketch_id)?;
    (!sketch.profiles.is_empty()).then_some(())?;
    let entities = ir
        .model
        .sketch_entities
        .iter()
        .filter(|entity| entity.sketch == *sketch_id)
        .map(|entity| (entity.id.clone(), entity))
        .collect::<BTreeMap<_, _>>();
    let mut profiles = Vec::new();
    for profile in &sketch.profiles {
        let mut geometries = Vec::new();
        for entity_use in profile {
            let entity = entities.get(&entity_use.entity)?;
            let (mut start, mut end) = sketch_geometry_endpoints(&entity.geometry)?;
            if entity_use.reversed {
                std::mem::swap(&mut start, &mut end);
            }
            geometries.push((entity.geometry.clone(), entity_use.reversed, start, end));
        }
        (geometries.len() >= minimum_entity_count).then_some(())?;
        let scale = geometries
            .iter()
            .flat_map(|(_, _, start, end)| start.iter().chain(end))
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        geometries
            .iter()
            .enumerate()
            .all(|(index, (_, _, _, end))| {
                let next = geometries[(index + 1) % geometries.len()].2;
                (end[0] - next[0]).hypot(end[1] - next[1]) <= 1e-9 * scale
            })
            .then_some(())?;
        profiles.push(geometries);
    }
    Some(profiles)
}

pub(super) fn profile_arc(
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
) -> Option<([f64; 2], f64, f64, f64)> {
    let (center, radius, start, forward_delta) = match &segment.0 {
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (
            [center.u, center.v],
            radius.0,
            (segment.2[1] - center.v).atan2(segment.2[0] - center.u),
            forward_arc_sweep(start_angle.0, end_angle.0),
        ),
        SketchGeometry::Circle { center, radius } => {
            ([center.u, center.v], radius.0, 0.0, std::f64::consts::TAU)
        }
        _ => return None,
    };
    let delta = if segment.1 {
        -forward_delta
    } else {
        forward_delta
    };
    Some((center, radius, start, delta))
}

pub(super) fn oriented_full_turn_angles(reversed: bool) -> [f64; 2] {
    if reversed {
        [std::f64::consts::TAU, 0.0]
    } else {
        [0.0, std::f64::consts::TAU]
    }
}

pub(super) fn segments_intersect(
    first: [[f64; 2]; 2],
    second: [[f64; 2]; 2],
    tolerance: f64,
) -> bool {
    let orient = |a: [f64; 2], b: [f64; 2], point: [f64; 2]| {
        (b[0] - a[0]).mul_add(point[1] - a[1], -((b[1] - a[1]) * (point[0] - a[0])))
    };
    let on_segment = |segment: [[f64; 2]; 2], point: [f64; 2]| {
        point[0] >= segment[0][0].min(segment[1][0]) - tolerance
            && point[0] <= segment[0][0].max(segment[1][0]) + tolerance
            && point[1] >= segment[0][1].min(segment[1][1]) - tolerance
            && point[1] <= segment[0][1].max(segment[1][1]) + tolerance
    };
    let orientations = [
        orient(first[0], first[1], second[0]),
        orient(first[0], first[1], second[1]),
        orient(second[0], second[1], first[0]),
        orient(second[0], second[1], first[1]),
    ];
    let first_length = (first[1][0] - first[0][0]).hypot(first[1][1] - first[0][1]);
    let second_length = (second[1][0] - second[0][0]).hypot(second[1][1] - second[0][1]);
    let first_cross_tolerance = tolerance * first_length.max(1.0);
    let second_cross_tolerance = tolerance * second_length.max(1.0);
    let opposite = |left: f64, right: f64, cross_tolerance: f64| {
        (left > cross_tolerance && right < -cross_tolerance)
            || (left < -cross_tolerance && right > cross_tolerance)
    };
    if opposite(orientations[0], orientations[1], first_cross_tolerance)
        && opposite(orientations[2], orientations[3], second_cross_tolerance)
    {
        return true;
    }
    (orientations[0].abs() <= first_cross_tolerance && on_segment(first, second[0]))
        || (orientations[1].abs() <= first_cross_tolerance && on_segment(first, second[1]))
        || (orientations[2].abs() <= second_cross_tolerance && on_segment(second, first[0]))
        || (orientations[3].abs() <= second_cross_tolerance && on_segment(second, first[1]))
}

pub(super) fn point_on_profile_arc(
    point: [f64; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let (center, radius, start, delta) = arc;
    let relative = [point[0] - center[0], point[1] - center[1]];
    let distance = relative[0].hypot(relative[1]);
    if (distance - radius).abs() > tolerance {
        return false;
    }
    let angle = relative[1].atan2(relative[0]);
    let travel = if delta >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU)
    };
    travel <= delta.abs() + tolerance / radius.max(1.0)
}

pub(super) fn line_arc_intersect(
    line: [[f64; 2]; 2],
    arc: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let direction = [line[1][0] - line[0][0], line[1][1] - line[0][1]];
    let relative = [line[0][0] - arc.0[0], line[0][1] - arc.0[1]];
    let a = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    let b = 2.0 * direction[0].mul_add(relative[0], direction[1] * relative[1]);
    let c = relative[0].mul_add(relative[0], relative[1] * relative[1]) - arc.1 * arc.1;
    let discriminant = b.mul_add(b, -(4.0 * a * c));
    if a <= tolerance * tolerance || discriminant < -tolerance * tolerance {
        return false;
    }
    let root = discriminant.max(0.0).sqrt();
    [-root, root].into_iter().any(|signed_root| {
        let parameter = (-b + signed_root) / (2.0 * a);
        parameter >= -tolerance
            && parameter <= 1.0 + tolerance
            && point_on_profile_arc(
                [
                    line[0][0] + parameter * direction[0],
                    line[0][1] + parameter * direction[1],
                ],
                arc,
                tolerance,
            )
    })
}

pub(super) fn arcs_intersect(
    first: ([f64; 2], f64, f64, f64),
    second: ([f64; 2], f64, f64, f64),
    tolerance: f64,
) -> bool {
    let displacement = [second.0[0] - first.0[0], second.0[1] - first.0[1]];
    let distance = displacement[0].hypot(displacement[1]);
    if distance <= tolerance && (first.1 - second.1).abs() <= tolerance {
        let endpoints = |arc: ([f64; 2], f64, f64, f64)| {
            [
                [
                    arc.0[0] + arc.1 * arc.2.cos(),
                    arc.0[1] + arc.1 * arc.2.sin(),
                ],
                [
                    arc.0[0] + arc.1 * (arc.2 + arc.3).cos(),
                    arc.0[1] + arc.1 * (arc.2 + arc.3).sin(),
                ],
            ]
        };
        return endpoints(first)
            .into_iter()
            .any(|point| point_on_profile_arc(point, second, tolerance))
            || endpoints(second)
                .into_iter()
                .any(|point| point_on_profile_arc(point, first, tolerance));
    }
    if distance <= tolerance
        || distance > first.1 + second.1 + tolerance
        || distance < (first.1 - second.1).abs() - tolerance
    {
        return false;
    }
    let along = (first.1 * first.1 - second.1 * second.1 + distance * distance) / (2.0 * distance);
    let height_squared = first.1 * first.1 - along * along;
    if height_squared < -tolerance * tolerance {
        return false;
    }
    let base = [
        first.0[0] + along * displacement[0] / distance,
        first.0[1] + along * displacement[1] / distance,
    ];
    let height = height_squared.max(0.0).sqrt();
    let offset = [
        -height * displacement[1] / distance,
        height * displacement[0] / distance,
    ];
    [-1.0, 1.0].into_iter().any(|sign| {
        let point = [base[0] + sign * offset[0], base[1] + sign * offset[1]];
        point_on_profile_arc(point, first, tolerance)
            && point_on_profile_arc(point, second, tolerance)
    })
}

pub(super) fn planar_point_segment_distance(point: [f64; 2], segment: [[f64; 2]; 2]) -> f64 {
    let direction = [segment[1][0] - segment[0][0], segment[1][1] - segment[0][1]];
    let relative = [point[0] - segment[0][0], point[1] - segment[0][1]];
    let length_squared = direction[0].mul_add(direction[0], direction[1] * direction[1]);
    if length_squared == 0.0 {
        return relative[0].hypot(relative[1]);
    }
    let parameter = (relative[0].mul_add(direction[0], relative[1] * direction[1])
        / length_squared)
        .clamp(0.0, 1.0);
    let nearest = [
        segment[0][0] + parameter * direction[0],
        segment[0][1] + parameter * direction[1],
    ];
    (point[0] - nearest[0]).hypot(point[1] - nearest[1])
}

pub(super) const NURBS_AREA_GAUSS_NODES: [f64; 8] = [
    -0.960_289_856_497_536_3,
    -0.796_666_477_413_626_7,
    -0.525_532_409_916_329,
    -0.183_434_642_495_649_8,
    0.183_434_642_495_649_8,
    0.525_532_409_916_329,
    0.796_666_477_413_626_7,
    0.960_289_856_497_536_3,
];
pub(super) const NURBS_AREA_GAUSS_WEIGHTS: [f64; 8] = [
    0.101_228_536_290_376_3,
    0.222_381_034_453_374_5,
    0.313_706_645_877_887_3,
    0.362_683_783_378_362,
    0.362_683_783_378_362,
    0.313_706_645_877_887_3,
    0.222_381_034_453_374_5,
    0.101_228_536_290_376_3,
];

pub(super) struct NurbsProfileSpan<'a> {
    pub(super) carrier: &'a CurveGeometry,
    pub(super) start: f64,
    pub(super) end: f64,
    pub(super) start_point: [f64; 2],
    pub(super) end_point: [f64; 2],
    pub(super) tolerance: f64,
    pub(super) depth: usize,
}

pub(super) fn append_nurbs_profile_span(
    span: &NurbsProfileSpan<'_>,
    points: &mut Vec<[f64; 2]>,
) -> Option<()> {
    const MAX_DEPTH: usize = 24;
    const MAX_POINTS: usize = 262_145;
    (span.start.is_finite() && span.end.is_finite() && span.start < span.end).then_some(())?;
    let middle = span.start + (span.end - span.start) * 0.5;
    if middle == span.start || middle == span.end {
        (points.len() < MAX_POINTS).then_some(())?;
        points.push(span.end_point);
        return Some(());
    }
    let first_quarter = span.start + (span.end - span.start) * 0.25;
    let third_quarter = span.start + (span.end - span.start) * 0.75;
    let middle_point = cadmpeg_ir::eval::curve_point(span.carrier, middle)?;
    let first_quarter_point = cadmpeg_ir::eval::curve_point(span.carrier, first_quarter)?;
    let third_quarter_point = cadmpeg_ir::eval::curve_point(span.carrier, third_quarter)?;
    let middle_point = [middle_point.x, middle_point.y];
    let first_quarter_point = [first_quarter_point.x, first_quarter_point.y];
    let third_quarter_point = [third_quarter_point.x, third_quarter_point.y];
    let chord = [span.start_point, span.end_point];
    let flatness = planar_point_segment_distance(first_quarter_point, chord)
        .max(planar_point_segment_distance(middle_point, chord))
        .max(planar_point_segment_distance(third_quarter_point, chord));
    (flatness.is_finite() && span.tolerance.is_finite() && span.tolerance > 0.0).then_some(())?;
    if flatness <= span.tolerance {
        (points.len() < MAX_POINTS).then_some(())?;
        points.push(span.end_point);
        return Some(());
    }
    (span.depth < MAX_DEPTH).then_some(())?;
    append_nurbs_profile_span(
        &NurbsProfileSpan {
            carrier: span.carrier,
            start: span.start,
            end: middle,
            start_point: span.start_point,
            end_point: middle_point,
            tolerance: span.tolerance,
            depth: span.depth + 1,
        },
        points,
    )?;
    append_nurbs_profile_span(
        &NurbsProfileSpan {
            carrier: span.carrier,
            start: middle,
            end: span.end,
            start_point: middle_point,
            end_point: span.end_point,
            tolerance: span.tolerance,
            depth: span.depth + 1,
        },
        points,
    )
}

pub(super) fn nurbs_profile_polyline(nurbs: &NurbsCurve, tolerance: f64) -> Option<Vec<[f64; 2]>> {
    let [lower, upper] = nurbs_intrinsic_parameter_range(nurbs)?;
    let carrier = CurveGeometry::Nurbs(nurbs.clone());
    let first = cadmpeg_ir::eval::curve_point(&carrier, lower)?;
    let first = [first.x, first.y];
    let mut points = vec![first];
    for pair in nurbs.knots.windows(2) {
        let start = pair[0].max(lower);
        let end = pair[1].min(upper);
        if start >= end {
            continue;
        }
        let start_point = cadmpeg_ir::eval::curve_point(&carrier, start)?;
        let end_point = cadmpeg_ir::eval::curve_point(&carrier, end)?;
        let start_point = [start_point.x, start_point.y];
        let end_point = [end_point.x, end_point.y];
        if points.last().copied() != Some(start_point) {
            points.push(start_point);
        }
        append_nurbs_profile_span(
            &NurbsProfileSpan {
                carrier: &carrier,
                start,
                end,
                start_point,
                end_point,
                tolerance,
                depth: 0,
            },
            &mut points,
        )?;
    }
    (points.len() >= 2 && points.iter().flatten().all(|value| value.is_finite())).then_some(points)
}

pub(super) fn profile_nurbs_polyline(
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    tolerance: f64,
) -> Option<Vec<[f64; 2]>> {
    let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
    nurbs_profile_polyline(&nurbs, tolerance)
}

pub(super) fn nurbs_profile_signed_area_twice(
    geometry: &SketchGeometry,
    reversed: bool,
) -> Option<f64> {
    let nurbs = oriented_sketch_nurbs_curve(geometry, reversed)?;
    let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
    let carrier = CurveGeometry::Nurbs(nurbs.clone());
    let mut area_twice = 0.0;
    for pair in nurbs.knots.windows(2) {
        let start = pair[0].max(lower);
        let end = pair[1].min(upper);
        if start >= end {
            continue;
        }
        let middle = 0.5 * (start + end);
        let half_width = 0.5 * (end - start);
        for (node, weight) in NURBS_AREA_GAUSS_NODES
            .into_iter()
            .zip(NURBS_AREA_GAUSS_WEIGHTS)
        {
            let parameter = middle + half_width * node;
            let point = cadmpeg_ir::eval::curve_point(&carrier, parameter)?;
            let tangent = cadmpeg_ir::eval::curve_tangent(&carrier, parameter)?;
            area_twice += weight * (point.x * tangent.y - point.y * tangent.x) * half_width;
        }
    }
    area_twice.is_finite().then_some(area_twice)
}

pub(super) fn polylines_intersect(first: &[[f64; 2]], second: &[[f64; 2]], tolerance: f64) -> bool {
    first.windows(2).any(|first_segment| {
        second.windows(2).any(|second_segment| {
            segments_intersect(
                [first_segment[0], first_segment[1]],
                [second_segment[0], second_segment[1]],
                tolerance,
            )
        })
    })
}

pub(super) fn profile_segments_intersect(
    first: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    second: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    tolerance: f64,
) -> bool {
    let first_nurbs = matches!(first.0, SketchGeometry::Nurbs { .. });
    let second_nurbs = matches!(second.0, SketchGeometry::Nurbs { .. });
    if first_nurbs || second_nurbs {
        if first_nurbs {
            if let Some(arc) = profile_arc(second) {
                return profile_nurbs_polyline(first, tolerance).is_some_and(|polyline| {
                    polyline
                        .windows(2)
                        .any(|segment| line_arc_intersect([segment[0], segment[1]], arc, tolerance))
                });
            }
        }
        if second_nurbs {
            if let Some(arc) = profile_arc(first) {
                return profile_nurbs_polyline(second, tolerance).is_some_and(|polyline| {
                    polyline
                        .windows(2)
                        .any(|segment| line_arc_intersect([segment[0], segment[1]], arc, tolerance))
                });
            }
        }
        let Some(first_polyline) = (if first_nurbs {
            profile_nurbs_polyline(first, tolerance)
        } else {
            Some(vec![first.2, first.3])
        }) else {
            return true;
        };
        let Some(second_polyline) = (if second_nurbs {
            profile_nurbs_polyline(second, tolerance)
        } else {
            Some(vec![second.2, second.3])
        }) else {
            return true;
        };
        return polylines_intersect(&first_polyline, &second_polyline, tolerance);
    }
    match (profile_arc(first), profile_arc(second)) {
        (None, None) => segments_intersect([first.2, first.3], [second.2, second.3], tolerance),
        (None, Some(arc)) => line_arc_intersect([first.2, first.3], arc, tolerance),
        (Some(arc), None) => line_arc_intersect([second.2, second.3], arc, tolerance),
        (Some(first), Some(second)) => arcs_intersect(first, second, tolerance),
    }
}

pub(super) fn profile_strictly_contains(profile: &ExtrusionProfile, point: [f64; 2]) -> bool {
    let scale = profile
        .iter()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    let mut winding = 0.0;
    for segment in profile {
        let mut accumulate = |first: [f64; 2], second: [f64; 2]| {
            let first = [first[0] - point[0], first[1] - point[1]];
            let second = [second[0] - point[0], second[1] - point[1]];
            winding += first[0]
                .mul_add(second[1], -(first[1] * second[0]))
                .atan2(first[0].mul_add(second[0], first[1] * second[1]));
        };
        if matches!(segment.0, SketchGeometry::Nurbs { .. }) {
            let Some(polyline) = profile_nurbs_polyline(segment, tolerance) else {
                return false;
            };
            for pair in polyline.windows(2) {
                accumulate(pair[0], pair[1]);
            }
        } else if let Some((center, radius, start, delta)) = profile_arc(segment) {
            let pieces = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0) as usize;
            for piece in 0..pieces {
                let first = start + delta * piece as f64 / pieces as f64;
                let second = start + delta * (piece + 1) as f64 / pieces as f64;
                accumulate(
                    [
                        center[0] + radius * first.cos(),
                        center[1] + radius * first.sin(),
                    ],
                    [
                        center[0] + radius * second.cos(),
                        center[1] + radius * second.sin(),
                    ],
                );
            }
        } else {
            accumulate(segment.2, segment.3);
        }
    }
    winding.abs() > std::f64::consts::PI
}

pub(super) fn ordered_extrusion_profiles(
    mut profiles: Vec<ExtrusionProfile>,
) -> Option<(Vec<ExtrusionProfile>, f64)> {
    let scale = profiles
        .iter()
        .flatten()
        .flat_map(|(_, _, start, end)| start.iter().chain(end))
        .map(|value| value.abs())
        .fold(1.0, f64::max);
    let tolerance = 1e-9 * scale;
    for profile in &profiles {
        for first in 0..profile.len() {
            for second in first + 1..profile.len() {
                if second == first + 1 || (first == 0 && second + 1 == profile.len()) {
                    continue;
                }
                if profile_segments_intersect(&profile[first], &profile[second], tolerance) {
                    return None;
                }
            }
        }
    }
    for first in 0..profiles.len() {
        for second in first + 1..profiles.len() {
            for first_segment in &profiles[first] {
                for second_segment in &profiles[second] {
                    if profile_segments_intersect(first_segment, second_segment, tolerance) {
                        return None;
                    }
                }
            }
        }
    }
    let outer = profiles
        .iter()
        .enumerate()
        .filter(|(candidate, profile)| {
            profiles.iter().enumerate().all(|(index, inner)| {
                index == *candidate || profile_strictly_contains(profile, inner[0].2)
            })
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [outer] = outer.as_slice() else {
        return None;
    };
    for first in 0..profiles.len() {
        if first == *outer {
            continue;
        }
        for second in first + 1..profiles.len() {
            if second == *outer {
                continue;
            }
            if profile_strictly_contains(&profiles[first], profiles[second][0].2)
                || profile_strictly_contains(&profiles[second], profiles[first][0].2)
            {
                return None;
            }
        }
    }
    let outer_area = extrusion_profile_signed_area(&profiles[*outer])?;
    if profiles.iter().enumerate().any(|(index, profile)| {
        index != *outer
            && extrusion_profile_signed_area(profile)
                .is_none_or(|area| area.is_sign_positive() == outer_area.is_sign_positive())
    }) {
        return None;
    }
    profiles.swap(0, *outer);
    Some((profiles, outer_area))
}

pub(super) fn add_extrusion_pcurve(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    id: PcurveId,
    source_offset: usize,
    geometry: PcurveGeometry,
) -> PcurveId {
    let parameter_range = match &geometry {
        PcurveGeometry::Nurbs {
            degree,
            knots,
            control_points,
            ..
        } => usize::try_from(*degree)
            .ok()
            .and_then(|degree| {
                (control_points.len() > degree && knots.len() == control_points.len() + degree + 1)
                    .then_some(())?;
                Some([*knots.get(degree)?, *knots.get(control_points.len())?])
            })
            .filter(|range| range[0] < range[1])
            .unwrap_or([0.0, 1.0]),
        _ => [0.0, 1.0],
    };
    annotate(
        annotations,
        &id,
        "FeatDefs",
        source_offset as u64,
        "extrusion_trim_pcurve",
        Exactness::Derived,
    );
    ir.model.pcurves.push(Pcurve {
        id: id.clone(),
        geometry,
        wrapper_reversed: None,
        native_tail_flags: None,
        parameter_range: Some(parameter_range),
        fit_tolerance: None,
    });
    id
}

pub(super) fn revolution_boundary_pcurve(
    surface: &SurfaceGeometry,
    point: [f64; 3],
    axis: RevolutionAxis,
) -> Option<PcurveGeometry> {
    let axis_direction = normalized([axis.direction.x, axis.direction.y, axis.direction.z])?;
    let axis_origin = [axis.origin.x, axis.origin.y, axis.origin.z];
    let point_from = |origin: Point3| {
        [
            point[0] - origin.x,
            point[1] - origin.y,
            point[2] - origin.z,
        ]
    };
    let vector = |value: Vector3| [value.x, value.y, value.z];
    let azimuth = |relative: [f64; 3], carrier_axis: [f64; 3], reference: [f64; 3]| {
        let tangent = cross(carrier_axis, reference);
        dot(relative, tangent).atan2(dot(relative, reference))
    };
    match surface {
        SurfaceGeometry::Plane {
            origin,
            normal,
            u_axis,
        } => {
            let normal = vector(*normal);
            let u_axis = vector(*u_axis);
            let v_axis = cross(normal, u_axis);
            let axis_relative = [
                axis_origin[0] - origin.x,
                axis_origin[1] - origin.y,
                axis_origin[2] - origin.z,
            ];
            let center = [dot(axis_relative, u_axis), dot(axis_relative, v_axis)];
            let relative = point_from(*origin);
            let uv = [dot(relative, u_axis), dot(relative, v_axis)];
            let radial = [uv[0] - center[0], uv[1] - center[1]];
            let radius = radial[0].hypot(radial[1]);
            (radius > 1e-12).then_some(())?;
            let start = radial[1].atan2(radial[0]);
            let direction = if dot(normal, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(circular_pcurve(center, radius, start, start + direction))
        }
        SurfaceGeometry::Cylinder {
            origin,
            axis,
            ref_direction,
            ..
        }
        | SurfaceGeometry::Cone {
            origin,
            axis,
            ref_direction,
            ..
        } => {
            let carrier_axis = vector(*axis);
            let relative = point_from(*origin);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let v = dot(relative, carrier_axis);
            let direction = if dot(carrier_axis, axis_direction).is_sign_negative() {
                -std::f64::consts::TAU
            } else {
                std::f64::consts::TAU
            };
            Some(line_pcurve([u, v], [u + direction, v]))
        }
        SurfaceGeometry::Sphere {
            center,
            axis,
            ref_direction,
            ..
        } => {
            let carrier_axis = vector(*axis);
            let relative = point_from(*center);
            let u = azimuth(relative, carrier_axis, vector(*ref_direction));
            let axial = dot(relative, carrier_axis);
            let radial = std::array::from_fn::<_, 3, _>(|index| {
                relative[index] - axial * carrier_axis[index]
            });
            let v = axial.atan2(dot(radial, radial).sqrt());
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v]))
        }
        SurfaceGeometry::Torus {
            center,
            axis,
            ref_direction,
            major_radius,
            minor_radius,
        } => {
            let carrier_axis = vector(*axis);
            let reference = vector(*ref_direction);
            let relative = point_from(*center);
            let axial = dot(relative, carrier_axis);
            let radial = std::array::from_fn::<_, 3, _>(|index| {
                relative[index] - axial * carrier_axis[index]
            });
            let radial_distance = dot(radial, radial).sqrt();
            let positive_residual = ((radial_distance - major_radius)
                .mul_add(radial_distance - major_radius, axial * axial)
                - minor_radius * minor_radius)
                .abs();
            let negative_residual = ((-radial_distance - major_radius)
                .mul_add(-radial_distance - major_radius, axial * axial)
                - minor_radius * minor_radius)
                .abs();
            let base_u = azimuth(relative, carrier_axis, reference);
            let (u, signed_ring) = if negative_residual < positive_residual {
                (base_u + std::f64::consts::PI, -radial_distance)
            } else {
                (base_u, radial_distance)
            };
            let scale = minor_radius.abs().max(radial_distance).max(1.0);
            (positive_residual.min(negative_residual) <= 1e-9 * scale * scale).then_some(())?;
            let v = axial.atan2(signed_ring - major_radius);
            Some(line_pcurve([u, v], [u + std::f64::consts::TAU, v]))
        }
        SurfaceGeometry::Nurbs(_)
        | SurfaceGeometry::Polygonal { .. }
        | SurfaceGeometry::Procedural { .. }
        | SurfaceGeometry::Transformed { .. }
        | SurfaceGeometry::Unknown { .. } => None,
    }
}

pub(super) fn revolved_brep_surface(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SketchGeometry,
    reversed: bool,
    axis: RevolutionAxis,
) -> Option<SurfaceGeometry> {
    if matches!(geometry, SketchGeometry::Nurbs { .. }) {
        let directrix = oriented_sketch_nurbs_curve(geometry, reversed)?;
        return Some(SurfaceGeometry::Nurbs(revolved_nurbs_surface(
            &placed_section_nurbs(transform, &directrix),
            axis,
        )?));
    }
    revolved_section_surface(transform, geometry, axis)
}

pub(super) fn revolution_profile_boundary_pcurve(
    transform: &crate::placement::FeatureSectionTransform,
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    surface: &SurfaceGeometry,
    axis: RevolutionAxis,
    section_point: [f64; 2],
    at_start: bool,
) -> Option<PcurveGeometry> {
    if matches!(segment.0, SketchGeometry::Nurbs { .. }) {
        let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = if at_start { lower } else { upper };
        return Some(line_pcurve(
            [parameter, 0.0],
            [parameter, std::f64::consts::TAU],
        ));
    }
    revolution_boundary_pcurve(
        surface,
        section_point_in_model(transform, section_point),
        axis,
    )
}

pub(super) fn revolution_face_sense(
    transform: &crate::placement::FeatureSectionTransform,
    segment: &(SketchGeometry, bool, [f64; 2], [f64; 2]),
    surface: &SurfaceGeometry,
    axis: RevolutionAxis,
    profile_area: f64,
) -> Option<Sense> {
    let is_nurbs = matches!(segment.0, SketchGeometry::Nurbs { .. });
    let (point, tangent, pcurve_parameter, u_epsilon) = if is_nurbs {
        let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = lower + (upper - lower) * 0.5;
        let carrier = CurveGeometry::Nurbs(nurbs);
        let point = cadmpeg_ir::eval::curve_point(&carrier, parameter)?;
        let tangent = cadmpeg_ir::eval::curve_tangent(&carrier, parameter)?;
        (
            [point.x, point.y],
            [tangent.x, tangent.y],
            0.5,
            (upper - lower).abs() * 1e-6,
        )
    } else if let Some((center, radius, start, delta)) = profile_arc(segment) {
        let angle = start + 0.5 * delta;
        (
            [
                center[0] + radius * angle.cos(),
                center[1] + radius * angle.sin(),
            ],
            [-delta.signum() * angle.sin(), delta.signum() * angle.cos()],
            0.0,
            1e-6,
        )
    } else {
        (
            [
                0.5 * (segment.2[0] + segment.3[0]),
                0.5 * (segment.2[1] + segment.3[1]),
            ],
            [segment.3[0] - segment.2[0], segment.3[1] - segment.2[1]],
            0.0,
            1e-6,
        )
    };
    let outward = if profile_area.is_sign_positive() {
        [tangent[1], -tangent[0]]
    } else {
        [-tangent[1], tangent[0]]
    };
    let outward = normalized(std::array::from_fn(|index| {
        outward[0] * transform.u_axis[index] + outward[1] * transform.v_axis[index]
    }))?;
    let model_point = section_point_in_model(transform, point);
    let pcurve = if is_nurbs {
        let nurbs = oriented_sketch_nurbs_curve(&segment.0, segment.1)?;
        let [lower, upper] = nurbs_intrinsic_parameter_range(&nurbs)?;
        let parameter = lower + (upper - lower) * 0.5;
        line_pcurve([parameter, 0.0], [parameter, std::f64::consts::TAU])
    } else {
        revolution_boundary_pcurve(surface, model_point, axis)?
    };
    let uv = cadmpeg_ir::eval::pcurve_uv(&pcurve, pcurve_parameter)?;
    let before_u = cadmpeg_ir::eval::surface_point(surface, uv.u - u_epsilon, uv.v)?;
    let after_u = cadmpeg_ir::eval::surface_point(surface, uv.u + u_epsilon, uv.v)?;
    let before_v = cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v - 1e-6)?;
    let after_v = cadmpeg_ir::eval::surface_point(surface, uv.u, uv.v + 1e-6)?;
    let du = [
        after_u.x - before_u.x,
        after_u.y - before_u.y,
        after_u.z - before_u.z,
    ];
    let dv = [
        after_v.x - before_v.x,
        after_v.y - before_v.y,
        after_v.z - before_v.z,
    ];
    let carrier_normal = normalized(cross(du, dv))?;
    let alignment = dot(carrier_normal, outward);
    (alignment.abs() > 1e-8).then_some(())?;
    Some(if alignment.is_sign_positive() {
        Sense::Forward
    } else {
        Sense::Reversed
    })
}

pub(super) fn transfer_resolved_revolution_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if current_additive_feature_recipe(&scan.features.operations, feature_id)
            != Some(crate::feature::FeatureRecipeKind::Revolve)
            || !feature_is_first_material_operation(scan, feature_id)
            || unique_feature_revolution_extent_kind(&scan.features.revolution_extents, feature_id)
                != Some(crate::feature::FeatureRevolutionExtentKind::FullTurn)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let extent = feature_revolution_extent(scan, feature_id);
        let Some(axis) = revolution_axis_for_transfer(
            scan,
            ir,
            feature_id,
            definition,
            transform,
            extent.as_ref(),
        ) else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some(mut profiles) = resolved_sketch_profiles(ir, &sketch_id, 2) else {
            continue;
        };
        let [profile] = profiles.as_mut_slice() else {
            continue;
        };
        let Some(area) = extrusion_profile_signed_area(profile) else {
            continue;
        };
        let vertex_curves = profile
            .iter()
            .map(|(_, _, point, _)| revolved_section_circle(transform, *point, axis))
            .collect::<Vec<_>>();
        let surface_geometries = profile
            .iter()
            .map(|(geometry, reversed, _, _)| {
                revolved_brep_surface(transform, geometry, *reversed, axis)
            })
            .collect::<Option<Vec<_>>>();
        let Some(surface_geometries) = surface_geometries else {
            continue;
        };
        let boundaries_are_complete = profile.iter().enumerate().all(|(index, segment)| {
            let next = (index + 1) % profile.len();
            (vertex_curves[index].is_some() || vertex_curves[next].is_some())
                && [
                    (segment.2, vertex_curves[index].is_some(), true),
                    (segment.3, vertex_curves[next].is_some(), false),
                ]
                .into_iter()
                .all(|(section_point, present, at_start)| {
                    !present
                        || revolution_profile_boundary_pcurve(
                            transform,
                            segment,
                            &surface_geometries[index],
                            axis,
                            section_point,
                            at_start,
                        )
                        .is_some()
                })
        });
        if !boundaries_are_complete {
            continue;
        }
        let face_senses = profile
            .iter()
            .zip(&surface_geometries)
            .map(|(segment, surface)| {
                revolution_face_sense(transform, segment, surface, axis, area)
            })
            .collect::<Option<Vec<_>>>();
        let Some(face_senses) = face_senses else {
            continue;
        };
        let prefix = format!("creo:feature:revolution#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let count = profile.len();
        let mut edges = vec![None; count];
        for (index, ((_, _, point, _), curve_geometry)) in
            profile.iter().zip(vertex_curves).enumerate()
        {
            let Some(curve_geometry) = curve_geometry else {
                continue;
            };
            let CurveGeometry::Circle {
                center,
                axis: curve_axis,
                ref_direction,
                radius,
            } = curve_geometry
            else {
                unreachable!();
            };
            let curve_id = CurveId(format!("{prefix}:curve:vertex:{index}"));
            let point_id = PointId(format!("{prefix}:point:vertex:{index}"));
            let vertex_id = VertexId(format!("{prefix}:vertex:{index}"));
            let edge_id = EdgeId(format!("{prefix}:edge:vertex:{index}"));
            let position = section_point_in_model(transform, *point);
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Circle {
                    center,
                    axis: curve_axis,
                    ref_direction,
                    radius,
                },
                source_object: None,
            });
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: Point3::new(position[0], position[1], position[2]),
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: vertex_id.clone(),
                end: vertex_id,
                param_range: Some([0.0, std::f64::consts::TAU]),
                tolerance: None,
            });
            edges[index] = Some(edge_id);
        }
        let mut faces = Vec::new();
        for (index, (((_, _, start, end), surface_geometry), face_sense)) in profile
            .iter()
            .zip(surface_geometries)
            .zip(face_senses)
            .enumerate()
        {
            let next = (index + 1) % count;
            let surface_id = SurfaceId(format!("{prefix}:surface:{index}"));
            let face_id = FaceId(format!("{prefix}:face:{index}"));
            ir.model.surfaces.push(Surface {
                id: surface_id.clone(),
                geometry: surface_geometry.clone(),
                source_object: None,
            });
            let mut loops = Vec::new();
            for (boundary, vertex_index, section_point, sense) in [
                ("start", index, *start, Sense::Reversed),
                ("end", next, *end, Sense::Forward),
            ] {
                let Some(edge_id) = edges[vertex_index].clone() else {
                    continue;
                };
                let loop_id = LoopId(format!("{prefix}:loop:{index}:{boundary}"));
                let coedge_id = CoedgeId(format!("{prefix}:coedge:{index}:{boundary}"));
                let radial_index = if boundary == "start" {
                    (index + count - 1) % count
                } else {
                    next
                };
                let radial_boundary = if boundary == "start" { "end" } else { "start" };
                let pcurve_geometry = revolution_profile_boundary_pcurve(
                    transform,
                    &profile[index],
                    &surface_geometry,
                    axis,
                    section_point,
                    boundary == "start",
                )
                .expect("revolution boundary was prevalidated");
                let pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!("{prefix}:pcurve:{index}:{boundary}")),
                    transform.offset,
                    pcurve_geometry,
                );
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                    coedges: vec![coedge_id.clone()],
                    vertex_uses: Vec::new(),
                });
                ir.model.coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id,
                    next: coedge_id.clone(),
                    previous: coedge_id,
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{radial_index}:{radial_boundary}"
                    )),
                    sense,
                    pcurves: vec![PcurveUse {
                        pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
                loops.push(loop_id);
            }
            ir.model.faces.push(Face {
                id: face_id.clone(),
                shell: shell_id.clone(),
                surface: surface_id,
                sense: face_sense,
                loops,
                name: None,
                color: None,
                tolerance: None,
            });
            faces.push(face_id);
        }
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn transfer_resolved_circular_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_additive_linear_extrusion(scan, feature_id)
            || !feature_is_first_material_operation(scan, feature_id)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some((section_center, radius)) =
            resolved_circular_extrusion_profile(scan, ir, transform, feature_id, &sketch_id)
        else {
            continue;
        };
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let prefix = format!("creo:feature:extrusion#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let center = section_point_in_model(transform, section_center);
        let seam =
            std::array::from_fn::<_, 3, _>(|axis| center[axis] + radius * transform.u_axis[axis]);
        let sides = [("bottom", span.lower), ("top", span.upper)];
        let mut face_ids = Vec::new();
        let mut cap_coedges = Vec::new();
        let mut side_coedges = Vec::new();
        for (side_index, (side, offset)) in sides.into_iter().enumerate() {
            let cap_surface = SurfaceId(format!("{prefix}:surface:{side}"));
            let cap_face = FaceId(format!("{prefix}:face:{side}"));
            let cap_loop = LoopId(format!("{prefix}:loop:{side}"));
            let curve_id = CurveId(format!("{prefix}:curve:{side}"));
            let edge_id = EdgeId(format!("{prefix}:edge:{side}"));
            let point_id = PointId(format!("{prefix}:point:{side}"));
            let vertex_id = VertexId(format!("{prefix}:vertex:{side}"));
            let cap_coedge = CoedgeId(format!("{prefix}:coedge:{side}:cap"));
            let side_coedge = CoedgeId(format!("{prefix}:coedge:{side}:side"));
            let cap_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId(format!("{prefix}:pcurve:{side}:cap")),
                transform.offset,
                circular_pcurve(section_center, radius, 0.0, std::f64::consts::TAU),
            );
            let side_pcurve = add_extrusion_pcurve(
                ir,
                annotations,
                PcurveId(format!("{prefix}:pcurve:{side}:side")),
                transform.offset,
                line_pcurve([0.0, offset], [std::f64::consts::TAU, offset]),
            );
            ir.model.surfaces.push(Surface {
                id: cap_surface.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(
                        transform.origin[0] + offset * transform.normal[0],
                        transform.origin[1] + offset * transform.normal[1],
                        transform.origin[2] + offset * transform.normal[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
                source_object: None,
            });
            ir.model.curves.push(Curve {
                id: curve_id.clone(),
                geometry: CurveGeometry::Circle {
                    center: Point3::new(
                        center[0] + offset * transform.normal[0],
                        center[1] + offset * transform.normal[1],
                        center[2] + offset * transform.normal[2],
                    ),
                    axis: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    ref_direction: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                    radius,
                },
                source_object: None,
            });
            ir.model.points.push(Point {
                id: point_id.clone(),
                position: Point3::new(
                    seam[0] + offset * transform.normal[0],
                    seam[1] + offset * transform.normal[1],
                    seam[2] + offset * transform.normal[2],
                ),
                source_object: None,
            });
            ir.model.vertices.push(Vertex {
                id: vertex_id.clone(),
                point: point_id,
                tolerance: None,
            });
            ir.model.edges.push(Edge {
                id: edge_id.clone(),
                curve: Some(curve_id),
                start: vertex_id.clone(),
                end: vertex_id,
                param_range: Some([0.0, std::f64::consts::TAU]),
                tolerance: None,
            });
            ir.model.loops.push(IrLoop {
                id: cap_loop.clone(),
                face: cap_face.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
                coedges: vec![cap_coedge.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: cap_coedge.clone(),
                owner_loop: cap_loop.clone(),
                edge: edge_id.clone(),
                next: cap_coedge.clone(),
                previous: cap_coedge.clone(),
                radial_next: side_coedge.clone(),
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                pcurves: vec![PcurveUse {
                    pcurve: cap_pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
            ir.model.faces.push(Face {
                id: cap_face.clone(),
                shell: shell_id.clone(),
                surface: cap_surface,
                sense: if side_index == 0 {
                    Sense::Reversed
                } else {
                    Sense::Forward
                },
                loops: vec![cap_loop],
                name: None,
                color: None,
                tolerance: None,
            });
            face_ids.push(cap_face);
            cap_coedges.push(cap_coedge);
            side_coedges.push((side_coedge, edge_id, side_pcurve));
        }
        let side_surface = SurfaceId(format!("{prefix}:surface:side"));
        let side_face = FaceId(format!("{prefix}:face:side"));
        let mut side_loops = Vec::new();
        ir.model.surfaces.push(Surface {
            id: side_surface.clone(),
            geometry: SurfaceGeometry::Cylinder {
                origin: Point3::new(center[0], center[1], center[2]),
                axis: Vector3::new(
                    transform.normal[0],
                    transform.normal[1],
                    transform.normal[2],
                ),
                ref_direction: Vector3::new(
                    transform.u_axis[0],
                    transform.u_axis[1],
                    transform.u_axis[2],
                ),
                radius,
            },
            source_object: None,
        });
        for (side_index, ((side, _), (coedge, edge, pcurve))) in
            sides.into_iter().zip(side_coedges).enumerate()
        {
            let loop_id = LoopId(format!("{prefix}:loop:side:{side}"));
            ir.model.loops.push(IrLoop {
                id: loop_id.clone(),
                face: side_face.clone(),
                boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Unspecified,
                coedges: vec![coedge.clone()],
                vertex_uses: Vec::new(),
            });
            ir.model.coedges.push(Coedge {
                id: coedge.clone(),
                owner_loop: loop_id.clone(),
                edge,
                next: coedge.clone(),
                previous: coedge.clone(),
                radial_next: cap_coedges[side_index].clone(),
                sense: if side_index == 0 {
                    Sense::Forward
                } else {
                    Sense::Reversed
                },
                pcurves: vec![PcurveUse {
                    pcurve,
                    isoparametric: None,
                    parameter_range: None,
                }],
                use_curve: None,
                use_curve_parameter_range: None,
            });
            side_loops.push(loop_id);
        }
        ir.model.faces.push(Face {
            id: side_face.clone(),
            shell: shell_id.clone(),
            surface: side_surface,
            sense: Sense::Forward,
            loops: side_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        face_ids.push(side_face);
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: face_ids,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}

pub(super) fn resolved_circular_extrusion_profile(
    scan: &ContainerScan,
    ir: &CadIr,
    transform: &crate::placement::FeatureSectionTransform,
    feature_id: u32,
    sketch_id: &SketchId,
) -> Option<([f64; 2], f64)> {
    if let Some(sketch) = ir
        .model
        .sketches
        .iter()
        .find(|sketch| sketch.id == *sketch_id)
    {
        if let [profile] = sketch.profiles.as_slice() {
            if let [entity_use] = profile.as_slice() {
                if let Some(SketchGeometry::Circle { center, radius }) = ir
                    .model
                    .sketch_entities
                    .iter()
                    .find(|entity| entity.id == entity_use.entity && entity.sketch == *sketch_id)
                    .map(|entity| &entity.geometry)
                {
                    return Some(([center.u, center.v], radius.0));
                }
            }
        }
    }
    let sweep = circular_sweep_geometry(scan, feature_id)?;
    sweep
        .section_definition_id
        .is_none_or(|definition_id| definition_id == transform.definition_id)
        .then_some(())?;
    circular_section_profile_from_cylinder(transform, &sweep.geometry)
}

pub(super) fn circular_section_profile_from_cylinder(
    transform: &crate::placement::FeatureSectionTransform,
    geometry: &SurfaceGeometry,
) -> Option<([f64; 2], f64)> {
    let SurfaceGeometry::Cylinder {
        origin,
        axis,
        radius,
        ..
    } = geometry
    else {
        return None;
    };
    let axis = normalized([axis.x, axis.y, axis.z])?;
    (dot(axis, transform.normal).abs() >= 1.0 - 1e-9 && radius.is_finite() && *radius > 0.0)
        .then_some(())?;
    let delta = [
        origin.x - transform.origin[0],
        origin.y - transform.origin[1],
        origin.z - transform.origin[2],
    ];
    Some((
        [dot(delta, transform.u_axis), dot(delta, transform.v_axis)],
        *radius,
    ))
}

pub(super) fn sketch_profiles_cover_generated_extrusion_sides(
    scan: &ContainerScan,
    definition: &crate::feature::FeatureDefinition,
    feature_id: u32,
    sketch: &Sketch,
) -> bool {
    let expected_entities = scan
        .features
        .entity_tables
        .iter()
        .filter(|table| table.feature_id == Some(feature_id))
        .flat_map(|table| {
            table.entries.iter().filter(move |entry| {
                entry.class_id == 200 && table.surface_ids.contains(&entry.entity_id)
            })
        })
        .filter_map(|entry| {
            let external_id = entry.source_entity_id?;
            scan.surfaces
                .rows
                .iter()
                .any(|row| {
                    row.id == entry.entity_id
                        && row.feature_id == feature_id
                        && matches!(
                            row.kind,
                            crate::surface::SurfaceKind::Plane
                                | crate::surface::SurfaceKind::Cylinder
                                | crate::surface::SurfaceKind::Extrusion
                        )
                })
                .then(|| {
                    SketchEntityId(format!(
                        "creo:featdefs:sketch_entity#{}:{external_id}",
                        definition.id
                    ))
                })
        })
        .collect::<Vec<_>>();
    let expected = expected_entities.iter().cloned().collect::<BTreeSet<_>>();
    let profile_entities = sketch
        .profiles
        .iter()
        .flatten()
        .map(|entity_use| entity_use.entity.clone())
        .collect::<Vec<_>>();
    !expected.is_empty()
        && expected_entities.len() == expected.len()
        && profile_entities.len() == expected.len()
        && profile_entities.into_iter().collect::<BTreeSet<_>>() == expected
}

pub(super) fn transfer_resolved_extrusion_breps(
    scan: &ContainerScan,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
) -> usize {
    let mut transferred = 0;
    for transform in &scan.features.section_transforms {
        if unique_feature_section_transform(
            &scan.features.section_transforms,
            transform.definition_id,
            transform.offset,
        )
        .is_none()
        {
            continue;
        }
        let Some(feature_id) = transform.feature_id else {
            continue;
        };
        if !feature_allows_additive_linear_extrusion(scan, feature_id)
            || !feature_is_first_material_operation(scan, feature_id)
        {
            continue;
        }
        let Some(definition) =
            unique_feature_definition_for_transform(&scan.features.definitions, transform)
        else {
            continue;
        };
        let sketch_id = model_sketch_id(scan, definition);
        let Some(span) = resolved_feature_extrusion_span(scan, ir, definition, transform) else {
            continue;
        };
        let length = span.upper - span.lower;
        let Some(sketch) = ir
            .model
            .sketches
            .iter()
            .find(|sketch| sketch.id == sketch_id)
        else {
            continue;
        };
        if !sketch_profiles_cover_generated_extrusion_sides(scan, definition, feature_id, sketch) {
            continue;
        }
        let Some(profiles) = resolved_sketch_profiles(ir, &sketch_id, 1) else {
            continue;
        };
        let Some((profiles, outer_area)) = ordered_extrusion_profiles(profiles) else {
            continue;
        };
        if profiles.iter().flatten().any(|(geometry, _, start, end)| {
            matches!(geometry, SketchGeometry::Line { .. }) && start == end
        }) {
            continue;
        }
        if profiles
            .iter()
            .flatten()
            .any(|(geometry, reversed, start, end)| {
                extrusion_brep_side_surface(transform, geometry, *reversed, *start, *end, span)
                    .is_none()
            })
        {
            continue;
        }
        let forward_caps = outer_area > 0.0;

        let prefix = format!("creo:feature:extrusion#{feature_id}");
        let body_id = BodyId(format!("{prefix}:body"));
        if ir.model.bodies.iter().any(|body| body.id == body_id) {
            continue;
        }
        let region_id = RegionId(format!("{prefix}:region"));
        let shell_id = ShellId(format!("{prefix}:shell"));
        let bottom_surface = SurfaceId(format!("{prefix}:surface:bottom"));
        let top_surface = SurfaceId(format!("{prefix}:surface:top"));
        for (id, offset) in [(&bottom_surface, span.lower), (&top_surface, span.upper)] {
            annotate(
                annotations,
                id,
                "FeatDefs",
                transform.offset as u64,
                "extrusion_cap_plane",
                Exactness::Derived,
            );
            ir.model.surfaces.push(Surface {
                id: id.clone(),
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(
                        transform.origin[0] + offset * transform.normal[0],
                        transform.origin[1] + offset * transform.normal[1],
                        transform.origin[2] + offset * transform.normal[2],
                    ),
                    normal: Vector3::new(
                        transform.normal[0],
                        transform.normal[1],
                        transform.normal[2],
                    ),
                    u_axis: Vector3::new(
                        transform.u_axis[0],
                        transform.u_axis[1],
                        transform.u_axis[2],
                    ),
                },
                source_object: None,
            });
        }

        let bottom_face = FaceId(format!("{prefix}:face:bottom"));
        let top_face = FaceId(format!("{prefix}:face:top"));
        let mut shell_faces = vec![bottom_face.clone(), top_face.clone()];
        let mut bottom_loops = Vec::new();
        let mut top_loops = Vec::new();
        for (profile_index, profile) in profiles.iter().enumerate() {
            let count = profile.len();
            let mut bottom_vertices = Vec::new();
            let mut top_vertices = Vec::new();
            for (index, (_, _, start, _)) in profile.iter().enumerate() {
                for (side, offset, arena) in [
                    ("bottom", span.lower, &mut bottom_vertices),
                    ("top", span.upper, &mut top_vertices),
                ] {
                    let position = section_point_in_model(transform, *start);
                    let point_id =
                        PointId(format!("{prefix}:point:{profile_index}:{index}:{side}"));
                    let vertex_id =
                        VertexId(format!("{prefix}:vertex:{profile_index}:{index}:{side}"));
                    ir.model.points.push(Point {
                        id: point_id.clone(),
                        position: Point3::new(
                            position[0] + offset * transform.normal[0],
                            position[1] + offset * transform.normal[1],
                            position[2] + offset * transform.normal[2],
                        ),
                        source_object: None,
                    });
                    ir.model.vertices.push(Vertex {
                        id: vertex_id.clone(),
                        point: point_id,
                        tolerance: None,
                    });
                    arena.push(vertex_id);
                }
            }

            let mut bottom_edges = Vec::new();
            let mut top_edges = Vec::new();
            let mut vertical_edges = Vec::new();
            for (index, (geometry, reversed, start, end)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                for (side, offset, vertices, arena) in [
                    ("bottom", span.lower, &bottom_vertices, &mut bottom_edges),
                    ("top", span.upper, &top_vertices, &mut top_edges),
                ] {
                    let curve_id =
                        CurveId(format!("{prefix}:curve:{profile_index}:{index}:{side}"));
                    let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:{side}"));
                    let curve = match geometry {
                        SketchGeometry::Line { .. } => {
                            let placed_start = section_point_in_model(transform, *start);
                            let placed_end = section_point_in_model(transform, *end);
                            let Some(direction) = normalized(std::array::from_fn(|axis| {
                                placed_end[axis] - placed_start[axis]
                            })) else {
                                continue;
                            };
                            CurveGeometry::Line {
                                origin: Point3::new(
                                    placed_start[0] + offset * transform.normal[0],
                                    placed_start[1] + offset * transform.normal[1],
                                    placed_start[2] + offset * transform.normal[2],
                                ),
                                direction: Vector3::new(direction[0], direction[1], direction[2]),
                            }
                        }
                        SketchGeometry::Arc { center, radius, .. }
                        | SketchGeometry::Circle { center, radius } => {
                            let center = section_point_in_model(transform, [center.u, center.v]);
                            let (axis_sign, _) = oriented_arc_parameterization(*reversed, 0.0, 0.0);
                            CurveGeometry::Circle {
                                center: Point3::new(
                                    center[0] + offset * transform.normal[0],
                                    center[1] + offset * transform.normal[1],
                                    center[2] + offset * transform.normal[2],
                                ),
                                axis: Vector3::new(
                                    axis_sign * transform.normal[0],
                                    axis_sign * transform.normal[1],
                                    axis_sign * transform.normal[2],
                                ),
                                ref_direction: Vector3::new(
                                    transform.u_axis[0],
                                    transform.u_axis[1],
                                    transform.u_axis[2],
                                ),
                                radius: radius.0,
                            }
                        }
                        SketchGeometry::Nurbs { .. } => {
                            let Some(nurbs) = oriented_sketch_nurbs_curve(geometry, *reversed)
                            else {
                                continue;
                            };
                            let placed = placed_section_nurbs(transform, &nurbs);
                            let translated = translated_nurbs_curve(
                                &placed,
                                [
                                    offset * transform.normal[0],
                                    offset * transform.normal[1],
                                    offset * transform.normal[2],
                                ],
                            );
                            CurveGeometry::Nurbs(translated)
                        }
                        _ => unreachable!("profile family checked above"),
                    };
                    ir.model.curves.push(Curve {
                        id: curve_id.clone(),
                        geometry: curve,
                        source_object: None,
                    });
                    let param_range = match geometry {
                        SketchGeometry::Line { .. } => {
                            Some([0.0, (end[0] - start[0]).hypot(end[1] - start[1])])
                        }
                        SketchGeometry::Arc {
                            start_angle,
                            end_angle,
                            ..
                        } => Some(
                            oriented_arc_parameterization(*reversed, start_angle.0, end_angle.0).1,
                        ),
                        SketchGeometry::Circle { .. } => Some(
                            oriented_arc_parameterization(*reversed, 0.0, std::f64::consts::TAU).1,
                        ),
                        SketchGeometry::Nurbs { .. } => {
                            oriented_sketch_nurbs_curve(geometry, *reversed)
                                .and_then(|nurbs| nurbs_intrinsic_parameter_range(&nurbs))
                        }
                        _ => None,
                    };
                    ir.model.edges.push(Edge {
                        id: edge_id.clone(),
                        curve: Some(curve_id),
                        start: vertices[index].clone(),
                        end: vertices[next].clone(),
                        param_range,
                        tolerance: None,
                    });
                    arena.push(edge_id);
                }
                let curve_id = CurveId(format!("{prefix}:curve:{profile_index}:{index}:vertical"));
                let edge_id = EdgeId(format!("{prefix}:edge:{profile_index}:{index}:vertical"));
                let origin = section_point_in_model(transform, *start);
                ir.model.curves.push(Curve {
                    id: curve_id.clone(),
                    geometry: CurveGeometry::Line {
                        origin: Point3::new(
                            origin[0] + span.lower * transform.normal[0],
                            origin[1] + span.lower * transform.normal[1],
                            origin[2] + span.lower * transform.normal[2],
                        ),
                        direction: Vector3::new(
                            transform.normal[0],
                            transform.normal[1],
                            transform.normal[2],
                        ),
                    },
                    source_object: None,
                });
                ir.model.edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(curve_id),
                    start: bottom_vertices[index].clone(),
                    end: top_vertices[index].clone(),
                    param_range: Some([0.0, length]),
                    tolerance: None,
                });
                vertical_edges.push(edge_id);
            }

            let bottom_loop = LoopId(format!("{prefix}:loop:{profile_index}:bottom"));
            let top_loop = LoopId(format!("{prefix}:loop:{profile_index}:top"));
            bottom_loops.push(bottom_loop.clone());
            top_loops.push(top_loop.clone());
            let bottom_coedges = (0..count)
                .rev()
                .map(|index| {
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:bottom-cap"
                    ))
                })
                .collect::<Vec<_>>();
            let top_coedges = (0..count)
                .map(|index| CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:top-cap")))
                .collect::<Vec<_>>();
            ir.model.loops.push(IrLoop {
                id: bottom_loop.clone(),
                face: bottom_face.clone(),
                boundary_role: if profile_index == 0 {
                    cadmpeg_ir::topology::LoopBoundaryRole::Outer
                } else {
                    cadmpeg_ir::topology::LoopBoundaryRole::Inner
                },
                coedges: bottom_coedges.clone(),
                vertex_uses: Vec::new(),
            });
            ir.model.loops.push(IrLoop {
                id: top_loop.clone(),
                face: top_face.clone(),
                boundary_role: if profile_index == 0 {
                    cadmpeg_ir::topology::LoopBoundaryRole::Outer
                } else {
                    cadmpeg_ir::topology::LoopBoundaryRole::Inner
                },
                coedges: top_coedges.clone(),
                vertex_uses: Vec::new(),
            });
            for ring_index in 0..count {
                let edge_index = count - 1 - ring_index;
                let id = bottom_coedges[ring_index].clone();
                let (geometry, reversed, start, end) = &profile[edge_index];
                let bottom_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{edge_index}:bottom-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: bottom_loop.clone(),
                    edge: bottom_edges[edge_index].clone(),
                    next: bottom_coedges[(ring_index + 1) % count].clone(),
                    previous: bottom_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{edge_index}:side-bottom"
                    )),
                    sense: Sense::Reversed,
                    pcurves: vec![PcurveUse {
                        pcurve: bottom_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
                let id = top_coedges[ring_index].clone();
                let (geometry, reversed, start, end) = &profile[ring_index];
                let top_pcurve = add_extrusion_pcurve(
                    ir,
                    annotations,
                    PcurveId(format!(
                        "{prefix}:pcurve:{profile_index}:{ring_index}:top-cap"
                    )),
                    transform.offset,
                    extrusion_cap_pcurve(geometry, *reversed, *start, *end),
                );
                ir.model.coedges.push(Coedge {
                    id,
                    owner_loop: top_loop.clone(),
                    edge: top_edges[ring_index].clone(),
                    next: top_coedges[(ring_index + 1) % count].clone(),
                    previous: top_coedges[(ring_index + count - 1) % count].clone(),
                    radial_next: CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{ring_index}:side-top"
                    )),
                    sense: Sense::Forward,
                    pcurves: vec![PcurveUse {
                        pcurve: top_pcurve,
                        isoparametric: None,
                        parameter_range: None,
                    }],
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }

            let forward_sides = extrusion_profile_signed_area(profile)
                .expect("validated extrusion profile has nonzero area")
                > 0.0;
            for (index, (geometry, _, start, _)) in profile.iter().enumerate() {
                let next = (index + 1) % count;
                let surface_id =
                    SurfaceId(format!("{prefix}:surface:{profile_index}:side:{index}"));
                let Some(surface_geometry) = extrusion_brep_side_surface(
                    transform,
                    geometry,
                    profile[index].1,
                    *start,
                    profile[index].3,
                    span,
                ) else {
                    break;
                };
                ir.model.surfaces.push(Surface {
                    id: surface_id.clone(),
                    geometry: surface_geometry,
                    source_object: None,
                });
                let face_id = FaceId(format!("{prefix}:face:{profile_index}:side:{index}"));
                let loop_id = LoopId(format!("{prefix}:loop:{profile_index}:side:{index}"));
                let coedges = [
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-bottom"
                    )),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{next}:side-vertical-out"
                    )),
                    CoedgeId(format!("{prefix}:coedge:{profile_index}:{index}:side-top")),
                    CoedgeId(format!(
                        "{prefix}:coedge:{profile_index}:{index}:side-vertical-in"
                    )),
                ];
                ir.model.loops.push(IrLoop {
                    id: loop_id.clone(),
                    face: face_id.clone(),
                    boundary_role: cadmpeg_ir::topology::LoopBoundaryRole::Outer,
                    coedges: coedges.to_vec(),
                    vertex_uses: Vec::new(),
                });
                let edge_uses = [
                    (bottom_edges[index].clone(), Sense::Forward),
                    (vertical_edges[next].clone(), Sense::Forward),
                    (top_edges[index].clone(), Sense::Reversed),
                    (vertical_edges[index].clone(), Sense::Reversed),
                ];
                let side_uvs =
                    extrusion_side_uvs(geometry, profile[index].1, *start, profile[index].3, span);
                for use_index in 0..4 {
                    let radial_next = match use_index {
                        0 => bottom_coedges[count - 1 - index].clone(),
                        1 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{next}:side-vertical-in"
                        )),
                        2 => top_coedges[index].clone(),
                        3 => CoedgeId(format!(
                            "{prefix}:coedge:{profile_index}:{index}:side-vertical-out"
                        )),
                        _ => unreachable!(),
                    };
                    let pcurve = add_extrusion_pcurve(
                        ir,
                        annotations,
                        PcurveId(format!(
                            "{prefix}:pcurve:{profile_index}:{index}:side:{use_index}"
                        )),
                        transform.offset,
                        line_pcurve(side_uvs[use_index][0], side_uvs[use_index][1]),
                    );
                    ir.model.coedges.push(Coedge {
                        id: coedges[use_index].clone(),
                        owner_loop: loop_id.clone(),
                        edge: edge_uses[use_index].0.clone(),
                        next: coedges[(use_index + 1) % 4].clone(),
                        previous: coedges[(use_index + 3) % 4].clone(),
                        radial_next,
                        sense: edge_uses[use_index].1,
                        pcurves: vec![PcurveUse {
                            pcurve,
                            isoparametric: None,
                            parameter_range: None,
                        }],
                        use_curve: None,
                        use_curve_parameter_range: None,
                    });
                }
                ir.model.faces.push(Face {
                    id: face_id.clone(),
                    shell: shell_id.clone(),
                    surface: surface_id,
                    sense: if forward_sides {
                        Sense::Forward
                    } else {
                        Sense::Reversed
                    },
                    loops: vec![loop_id],
                    name: None,
                    color: None,
                    tolerance: None,
                });
                shell_faces.push(face_id);
            }
        }
        ir.model.faces.push(Face {
            id: bottom_face,
            shell: shell_id.clone(),
            surface: bottom_surface,
            sense: if forward_caps {
                Sense::Reversed
            } else {
                Sense::Forward
            },
            loops: bottom_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.faces.push(Face {
            id: top_face,
            shell: shell_id.clone(),
            surface: top_surface,
            sense: if forward_caps {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: top_loops,
            name: None,
            color: None,
            tolerance: None,
        });
        ir.model.shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: shell_faces,
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        ir.model.regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        ir.model.bodies.push(Body {
            id: body_id,
            kind: BodyKind::Solid,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        transferred += 1;
    }
    transferred
}
