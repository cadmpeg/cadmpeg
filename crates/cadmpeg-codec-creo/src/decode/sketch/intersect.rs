// SPDX-License-Identifier: Apache-2.0
//! Section carrier intersection, trim vertices, and coordinate reconciliation.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::{Angle, Length};
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::sketches::SketchGeometry;

pub(crate) use crate::vecmath::normalized;

use super::geometry::{
    resolved_section_segment_geometry_with_missing_line, saved_section_arc_carrier,
    saved_section_arc_record, saved_section_missing_line_geometry,
};
use super::radii::{
    resolved_section_radii, section_arc_carrier,
    section_segment_intersection_carrier_with_missing_line, trim_segment_id,
    SectionIntersectionCarrier,
};
use super::skamp::section_line_entity_fixed_coordinate_with_unique_rows;

pub(crate) fn section_line_origin_direction(geometry: &SketchGeometry) -> Option<(Point2, Point2)> {
    match geometry {
        SketchGeometry::Line { start, end } => {
            Some((*start, Point2::new(end.u - start.u, end.v - start.v)))
        }
        SketchGeometry::ReferenceLine { origin, direction } => Some((*origin, *direction)),
        _ => None,
    }
}

pub(crate) fn intersect_section_lines(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (first_origin, first_direction) = section_line_origin_direction(first)?;
    let (second_origin, second_direction) = section_line_origin_direction(second)?;
    let first_end = Point2::new(
        first_origin.u + first_direction.u,
        first_origin.v + first_direction.v,
    );
    let second_end = Point2::new(
        second_origin.u + second_direction.u,
        second_origin.v + second_direction.v,
    );
    let denominator = (first_origin.u - first_end.u).mul_add(
        second_origin.v - second_end.v,
        -(first_origin.v - first_end.v) * (second_origin.u - second_end.u),
    );
    let scale = (first_origin.u - first_end.u)
        .abs()
        .max((first_origin.v - first_end.v).abs())
        .max((second_origin.u - second_end.u).abs())
        .max((second_origin.v - second_end.v).abs())
        .max(1.0);
    if denominator.abs() <= 1e-12 * scale * scale {
        return None;
    }
    let first_cross = first_origin
        .u
        .mul_add(first_end.v, -(first_origin.v * first_end.u));
    let second_cross = second_origin
        .u
        .mul_add(second_end.v, -(second_origin.v * second_end.u));
    Some([
        first_cross.mul_add(
            second_origin.u - second_end.u,
            -(first_origin.u - first_end.u) * second_cross,
        ) / denominator,
        first_cross.mul_add(
            second_origin.v - second_end.v,
            -(first_origin.v - first_end.v) * second_cross,
        ) / denominator,
    ])
}

pub(crate) fn intersect_section_line_arc(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        (line @ SketchGeometry::Line { .. }, arc @ SketchGeometry::Arc { .. })
        | (arc @ SketchGeometry::Arc { .. }, line @ SketchGeometry::Line { .. }),
    ) = ((first, second),)
    else {
        return None;
    };
    let SketchGeometry::Line { start, end } = line else {
        return None;
    };
    let SketchGeometry::Arc { center, radius, .. } = arc else {
        return None;
    };
    let direction = [end.u - start.u, end.v - start.v];
    let length = direction[0].hypot(direction[1]);
    if length <= 1e-12 || radius.0 <= 1e-12 {
        return None;
    }
    let direction = direction.map(|value| value / length);
    let relative = [start.u - center.u, start.v - center.v];
    let projection = -(relative[0] * direction[0] + relative[1] * direction[1]);
    let closest = [
        start.u + projection * direction[0],
        start.v + projection * direction[1],
    ];
    let distance_squared = (closest[0] - center.u).mul_add(
        closest[0] - center.u,
        (closest[1] - center.v) * (closest[1] - center.v),
    );
    let radial_squared = radius.0 * radius.0;
    let scale = radial_squared.max(1.0);
    if distance_squared > radial_squared + 1e-10 * scale {
        return None;
    }
    let travel = (radial_squared - distance_squared).max(0.0).sqrt();
    let candidates = [
        [
            closest[0] + travel * direction[0],
            closest[1] + travel * direction[1],
        ],
        [
            closest[0] - travel * direction[0],
            closest[1] - travel * direction[1],
        ],
    ];
    if travel <= 1e-10 * radius.0.max(1.0) {
        let parameter = projection / length;
        return (-1e-10..=1.0 + 1e-10)
            .contains(&parameter)
            .then_some(candidates[0]);
    }
    let parameters = [
        (projection + travel) / length,
        (projection - travel) / length,
    ];
    let inside = parameters
        .into_iter()
        .enumerate()
        .filter(|(_, parameter)| (-1e-10..=1.0 + 1e-10).contains(parameter))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let [index] = inside.as_slice() else {
        return None;
    };
    Some(candidates[*index])
}

pub(crate) fn intersect_tangent_section_arcs(
    first: &SketchGeometry,
    second: &SketchGeometry,
) -> Option<[f64; 2]> {
    let (
        SketchGeometry::Arc {
            center: first_center,
            radius: first_radius,
            ..
        },
        SketchGeometry::Arc {
            center: second_center,
            radius: second_radius,
            ..
        },
    ) = (first, second)
    else {
        return None;
    };
    if first_radius.0 <= 1e-12 || second_radius.0 <= 1e-12 {
        return None;
    }
    let delta = [
        second_center.u - first_center.u,
        second_center.v - first_center.v,
    ];
    let distance = delta[0].hypot(delta[1]);
    let scale = distance.max(first_radius.0).max(second_radius.0).max(1.0);
    if distance <= 1e-12 * scale {
        return None;
    }
    let offset = (first_radius
        .0
        .mul_add(first_radius.0, -(second_radius.0 * second_radius.0))
        + distance * distance)
        / (2.0 * distance);
    let height_squared = first_radius.0.mul_add(first_radius.0, -(offset * offset));
    if height_squared.abs() > 1e-9 * scale * scale {
        return None;
    }
    Some([
        first_center.u + offset * delta[0] / distance,
        first_center.v + offset * delta[1] / distance,
    ])
}

pub(crate) fn intersect_section_carriers(
    first: &SectionIntersectionCarrier,
    second: &SectionIntersectionCarrier,
) -> Option<[f64; 2]> {
    let line_arc_is_bounded = matches!(
        (&first.geometry, &second.geometry),
        (SketchGeometry::Line { .. }, SketchGeometry::Arc { .. })
            | (SketchGeometry::Arc { .. }, SketchGeometry::Line { .. })
    );
    intersect_section_lines(&first.geometry, &second.geometry)
        .or_else(|| {
            line_arc_is_bounded
                .then(|| intersect_section_line_arc(&first.geometry, &second.geometry))
                .flatten()
        })
        .or_else(|| intersect_tangent_section_arcs(&first.geometry, &second.geometry))
}

pub(crate) fn intersect_incident_section_carriers(
    carriers: &[SectionIntersectionCarrier],
) -> Option<[f64; 2]> {
    (carriers.len() >= 2).then_some(())?;
    let mut candidates = Vec::new();
    for first in 0..carriers.len() {
        for second in first + 1..carriers.len() {
            candidates.push((
                0,
                intersect_section_carriers(&carriers[first], &carriers[second])?,
            ));
        }
    }
    let (coordinates, ambiguous) = reconciled_section_coordinates(candidates);
    ambiguous.is_empty().then_some(())?;
    coordinates.get(&0).copied()
}

pub(crate) fn resolved_trim_vertex_coordinates(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
) -> BTreeMap<u32, [f64; 2]> {
    let Some(segments) = &definition.segments else {
        return BTreeMap::new();
    };
    let radii = resolved_section_radii(definition);
    let missing_line = saved_section_missing_line_geometry(definition);
    let variable_points = definition
        .variables
        .as_ref()
        .map(|variables| variables.reconciled_points().0)
        .unwrap_or_default();
    let mut seen_vertex_ids = BTreeSet::new();
    let duplicate_vertex_ids = definition
        .trim_vertices
        .iter()
        .filter(|table| table.has_complete_bucket_frame())
        .flat_map(|table| &table.rows)
        .filter_map(|vertex| {
            (!seen_vertex_ids.insert(vertex.vertex_id)).then_some(vertex.vertex_id)
        })
        .collect::<BTreeSet<_>>();
    let mut coordinate_candidates = definition
        .trim_vertices
        .iter()
        .filter(|table| table.has_complete_bucket_frame())
        .flat_map(|table| &table.rows)
        .filter_map(|vertex| Some((vertex.vertex_id, vertex.section_coordinates?)))
        .collect::<Vec<_>>();
    for trim in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, trim) else {
            continue;
        };
        let Some(segment) = segments.unique_segment(external_id) else {
            continue;
        };
        let Some(([center_u, center_v], radius)) = saved_section_arc_carrier(definition, segment)
        else {
            continue;
        };
        let Some(arc) = saved_section_arc_record(definition, segment) else {
            continue;
        };
        for (vertex, endpoint) in trim.vertices.into_iter().zip(arc.endpoints) {
            let [Some(u), Some(v), _] = endpoint else {
                continue;
            };
            let candidate = [u, v];
            let candidate_radius = (u - center_u).hypot(v - center_v);
            let radial_scale = radius.max(candidate_radius).max(1.0);
            if (candidate_radius - radius).abs() > 1e-9 * radial_scale {
                continue;
            }
            coordinate_candidates.push((vertex, candidate));
        }
    }
    let mut incident = BTreeMap::<u32, Vec<u32>>::new();
    for entity in definition
        .trim_entities
        .iter()
        .flat_map(|table| &table.rows)
    {
        let Some(external_id) = trim_segment_id(definition, entity) else {
            continue;
        };
        for vertex in entity.vertices {
            incident.entry(vertex).or_default().push(external_id);
        }
    }
    let explicit_incident = definition
        .trim_vertices
        .as_ref()
        .filter(|table| table.has_complete_bucket_frame())
        .map(|table| {
            let mut result = BTreeMap::<u32, Vec<u32>>::new();
            for vertex in &table.rows {
                let mut resolved = Vec::new();
                for entity_id in &vertex.entities {
                    let matches = definition
                        .trim_entities
                        .iter()
                        .flat_map(|table| &table.rows)
                        .filter(|entity| entity.external_id == *entity_id)
                        .collect::<Vec<_>>();
                    let external_id = match matches.as_slice() {
                        [entity] => trim_segment_id(definition, entity),
                        [] => segments
                            .unique_segment(*entity_id)
                            .map(|segment| segment.external_id),
                        _ => None,
                    };
                    if let Some(external_id) = external_id {
                        resolved.push(external_id);
                    }
                }
                resolved.sort_unstable();
                if resolved.len() == vertex.entities.len() {
                    result.entry(vertex.vertex_id).or_default().extend(resolved);
                }
            }
            result
        });
    if let Some(explicit) = &explicit_incident {
        for (vertex, entities) in explicit {
            if entities.len() < 2 || entities.windows(2).any(|pair| pair[0] == pair[1]) {
                continue;
            }
            let mut derived = incident.get(vertex).cloned().unwrap_or_default();
            derived.sort_unstable();
            derived.dedup();
            if derived != *entities {
                continue;
            }
            incident.insert(*vertex, entities.clone());
        }
    }
    let intersection_carriers = incident
        .values()
        .flatten()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter_map(|external_id| {
            let segment = segments.unique_segment(external_id)?;
            let carrier = section_segment_intersection_carrier_with_missing_line(
                definition,
                &radii,
                points,
                segment,
                missing_line.as_ref(),
                &variable_points,
            )?;
            Some((external_id, carrier))
        })
        .collect::<BTreeMap<_, _>>();
    for (vertex, mut entities) in incident {
        entities.sort_unstable();
        if entities.len() < 2 || entities.windows(2).any(|pair| pair[0] == pair[1]) {
            continue;
        }
        if explicit_incident
            .as_ref()
            .is_some_and(|explicit| explicit.get(&vertex) != Some(&entities))
        {
            continue;
        }
        // A unique shared endpoint coordinate is a trim witness even when a
        // complete carrier cannot be evaluated from the remaining points.
        let common_points = entities
            .iter()
            .filter_map(|external_id| segments.unique_segment(*external_id))
            .map(|segment| segment.point_ids.into_iter().collect::<BTreeSet<_>>())
            .reduce(|common, points| common.intersection(&points).copied().collect());
        if let Some(common_points) = common_points {
            let common_points = common_points.into_iter().collect::<Vec<_>>();
            if let [point_id] = common_points.as_slice() {
                if let Some(coordinate) = points.get(point_id) {
                    coordinate_candidates.push((vertex, *coordinate));
                }
            }
        }
        let carriers = entities
            .iter()
            .map(|external_id| intersection_carriers.get(external_id).cloned())
            .collect::<Option<Vec<_>>>();
        let Some(carriers) = carriers else {
            continue;
        };
        if let Some(coordinate) = intersect_incident_section_carriers(&carriers) {
            coordinate_candidates.push((vertex, coordinate));
        }
    }
    let (mut coordinates, mut ambiguous_vertices) =
        reconciled_section_coordinates(coordinate_candidates);
    ambiguous_vertices.extend(duplicate_vertex_ids);
    coordinates.retain(|vertex, _| !ambiguous_vertices.contains(vertex));
    loop {
        let mut additions = Vec::new();
        for trim in definition
            .trim_entities
            .iter()
            .flat_map(|table| &table.rows)
        {
            let Some(external_id) = trim_segment_id(definition, trim) else {
                continue;
            };
            let Some(segment) = segments.unique_segment(external_id) else {
                continue;
            };
            let Some(SketchGeometry::Line { start, end }) =
                resolved_section_segment_geometry_with_missing_line(
                    definition,
                    points,
                    segment,
                    missing_line.as_ref(),
                )
            else {
                continue;
            };
            let stored = [[start.u, start.v], [end.u, end.v]];
            let known = trim
                .vertices
                .map(|vertex| coordinates.get(&vertex).copied());
            let (known_point, missing_index) = match known {
                [Some(point), None] => (point, 1),
                [None, Some(point)] => (point, 0),
                _ => continue,
            };
            let distances =
                stored.map(|point| (point[0] - known_point[0]).hypot(point[1] - known_point[1]));
            let scale = stored
                .iter()
                .flatten()
                .map(|value| value.abs())
                .fold(1.0, f64::max);
            let matched = if distances[0] <= 1e-9 * scale && distances[1] > 1e-9 * scale {
                0
            } else if distances[1] <= 1e-9 * scale && distances[0] > 1e-9 * scale {
                1
            } else {
                continue;
            };
            additions.push((trim.vertices[missing_index], stored[1 - matched]));
        }
        let (additions, conflicts) = reconciled_section_coordinates(additions);
        ambiguous_vertices.extend(conflicts);
        let mut changed = false;
        for (vertex, coordinate) in additions {
            if ambiguous_vertices.contains(&vertex) {
                continue;
            }
            if let std::collections::btree_map::Entry::Vacant(entry) = coordinates.entry(vertex) {
                entry.insert(coordinate);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    coordinates
}

pub(crate) fn reconciled_section_coordinates(
    candidates: impl IntoIterator<Item = (u32, [f64; 2])>,
) -> (BTreeMap<u32, [f64; 2]>, BTreeSet<u32>) {
    let mut grouped = BTreeMap::<u32, Vec<[f64; 2]>>::new();
    for (vertex, coordinate) in candidates {
        grouped.entry(vertex).or_default().push(coordinate);
    }
    let mut coordinates = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for (vertex, values) in grouped {
        let first = values[0];
        let scale = values
            .iter()
            .flatten()
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        if values.iter().all(|candidate| {
            (candidate[0] - first[0]).hypot(candidate[1] - first[1]) <= 1e-9 * scale
        }) {
            coordinates.insert(vertex, first);
        } else {
            ambiguous.insert(vertex);
        }
    }
    (coordinates, ambiguous)
}

pub(crate) fn trimmed_section_segment_geometry_with_missing_line(
    definition: &crate::feature::FeatureDefinition,
    points: &BTreeMap<u32, [f64; 2]>,
    trim_vertices: &BTreeMap<u32, [f64; 2]>,
    segment: &crate::feature::FeatureSegment,
    missing_line: Option<&(usize, SketchGeometry)>,
) -> Option<SketchGeometry> {
    let trim = definition
        .trim_entities
        .as_ref()?
        .rows
        .iter()
        .find(|row| trim_segment_id(definition, row) == Some(segment.external_id))?;
    let start = trim_vertices.get(&trim.vertices[0])?;
    let end = trim_vertices.get(&trim.vertices[1])?;
    if let Some(SketchGeometry::Line {
        start: carrier_start,
        end: carrier_end,
    }) = resolved_section_segment_geometry_with_missing_line(
        definition,
        points,
        segment,
        missing_line,
    ) {
        let scale = [
            carrier_start.u,
            carrier_start.v,
            carrier_end.u,
            carrier_end.v,
            start[0],
            start[1],
            end[0],
            end[1],
        ]
        .into_iter()
        .map(f64::abs)
        .fold(1.0, f64::max);
        let direction = [
            carrier_end.u / scale - carrier_start.u / scale,
            carrier_end.v / scale - carrier_start.v / scale,
        ];
        let direction_norm = direction[0].hypot(direction[1]);
        if direction_norm <= 1e-12
            || [start, end].into_iter().any(|point| {
                let offset = [
                    point[0] / scale - carrier_start.u / scale,
                    point[1] / scale - carrier_start.v / scale,
                ];
                (offset[0] * direction[1] - offset[1] * direction[0]).abs() > 1e-9 * direction_norm
            })
        {
            return None;
        }
    } else if let Some(([center_u, center_v], radius)) =
        section_arc_carrier(&resolved_section_radii(definition), points, segment)
            .or_else(|| saved_section_arc_carrier(definition, segment))
    {
        let first = [start[0] - center_u, start[1] - center_v];
        let second = [end[0] - center_u, end[1] - center_v];
        let first_radius = first[0].hypot(first[1]);
        let second_radius = second[0].hypot(second[1]);
        let scale = radius.max(first_radius).max(second_radius).max(1.0);
        if (first_radius - radius).abs() > 1e-9 * scale
            || (second_radius - radius).abs() > 1e-9 * scale
        {
            return None;
        }
        let start_angle = second[1].atan2(second[0]);
        let mut end_angle = first[1].atan2(first[0]);
        while end_angle <= start_angle {
            end_angle += std::f64::consts::TAU;
        }
        return Some(SketchGeometry::Arc {
            center: cadmpeg_ir::math::Point2::new(center_u, center_v),
            radius: Length(radius),
            start_angle: Angle(start_angle),
            end_angle: Angle(end_angle),
        });
    } else {
        let scale = start
            .iter()
            .chain(end)
            .map(|value| value.abs())
            .fold(1.0, f64::max);
        let orientation_matches = match section_line_entity_fixed_coordinate_with_unique_rows(
            definition,
            segment.external_id,
        ) {
            Some(0) => (start[0] - end[0]).abs() <= 1e-9 * scale,
            Some(1) => (start[1] - end[1]).abs() <= 1e-9 * scale,
            _ => false,
        };
        orientation_matches.then_some(())?;
    }
    Some(SketchGeometry::Line {
        start: cadmpeg_ir::math::Point2::new(start[0], start[1]),
        end: cadmpeg_ir::math::Point2::new(end[0], end[1]),
    })
}

pub(crate) fn section_point_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 2],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
    })
}

pub(crate) fn section_xyz_in_model(
    transform: &crate::placement::FeatureSectionTransform,
    point: [f64; 3],
) -> [f64; 3] {
    std::array::from_fn(|axis| {
        transform.origin[axis]
            + point[0] * transform.u_axis[axis]
            + point[1] * transform.v_axis[axis]
            + point[2] * transform.normal[axis]
    })
}

#[cfg(test)]
mod tests {
    use super::{
        resolved_trim_vertex_coordinates, trimmed_section_segment_geometry_with_missing_line,
    };
    use cadmpeg_ir::math::Point2;
    use cadmpeg_ir::sketches::SketchGeometry;
    use std::collections::BTreeMap;

    #[test]
    fn trim_vertex_requires_exact_trim_entity_incidence() {
        let segment = |external_id, point_ids| crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids,
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: None,
            radius_ref: None,
            radius2_ref: None,
            external_id,
            body: Vec::new(),
            offset: 0,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 1,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count: 2,
                has_elided_prototype: false,
                entity_ref: None,
                rows: vec![segment(42, [1, 2]), segment(43, [3, 4])],
                circle_rows: Vec::new(),
                point_rows: Vec::new(),
                centered_line_rows: Vec::new(),
                reference_line_rows: Vec::new(),
                bounded_curve_rows: Vec::new(),
                conic_rows: Vec::new(),
                opaque_rows: Vec::new(),
                offset: 0,
            }),
            trim_entities: Some(crate::feature::FeatureTrimEntityTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                buckets: Vec::new(),
                rows: vec![crate::feature::FeatureTrimEntity {
                    external_id: 42,
                    mode: None,
                    vertices: [1, 2],
                    center_vertex: None,
                    kind: crate::feature::TrimEntityKind::Line,
                    offset: 0,
                }],
                solved_external_ids: vec![42],
                offset: 0,
            }),
            trim_vertices: Some(crate::feature::FeatureTrimVertexTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                buckets: Vec::new(),
                rows: vec![crate::feature::FeatureTrimVertex {
                    vertex_id: 3,
                    entities: vec![42, 43],
                    section_coordinates: None,
                    offset: 0,
                }],
                offset: 0,
            }),
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };

        assert_eq!(
            resolved_trim_vertex_coordinates(
                &definition,
                &BTreeMap::from([
                    (1, [-1.0, 0.0]),
                    (2, [1.0, 0.0]),
                    (3, [0.0, -1.0]),
                    (4, [0.0, 1.0]),
                ]),
            ),
            BTreeMap::new()
        );

        let mut shared_point = definition.clone();
        shared_point.trim_vertices = None;
        shared_point.segments.as_mut().expect("segments").rows[1].point_ids = [2, 3];
        shared_point
            .trim_entities
            .as_mut()
            .expect("trim entities")
            .rows
            .push(crate::feature::FeatureTrimEntity {
                external_id: 43,
                mode: None,
                vertices: [2, 3],
                center_vertex: None,
                kind: crate::feature::TrimEntityKind::Line,
                offset: 0,
            });
        assert_eq!(
            resolved_trim_vertex_coordinates(&shared_point, &BTreeMap::from([(2, [0.0, 0.0])]),),
            BTreeMap::from([(2, [0.0, 0.0])])
        );
    }

    #[test]
    fn incomplete_unique_trim_line_uses_stored_orientation() {
        let segment = crate::feature::FeatureSegment {
            kind: crate::feature::FeatureSegmentKind::Line,
            directions: [None; 3],
            point_ids: [1, 2],
            center_id: None,
            arc_orientation: None,
            vertical_horizontal: Some(1),
            radius_ref: None,
            radius2_ref: None,
            external_id: 10,
            body: Vec::new(),
            offset: 0,
        };
        let definition = crate::feature::FeatureDefinition {
            id: 2,
            owner_feature_id: None,
            body: Vec::new(),
            parameter_frames: Vec::new(),
            outlines: Vec::new(),
            variables: None,
            segments: Some(crate::feature::FeatureSegmentTable {
                declared_count: 2,
                has_elided_prototype: false,
                entity_ref: None,
                rows: vec![segment.clone()],
                circle_rows: Vec::new(),
                point_rows: Vec::new(),
                centered_line_rows: Vec::new(),
                reference_line_rows: Vec::new(),
                bounded_curve_rows: Vec::new(),
                conic_rows: Vec::new(),
                opaque_rows: Vec::new(),
                offset: 0,
            }),
            trim_entities: Some(crate::feature::FeatureTrimEntityTable {
                declared_count: None,
                entity_ref: None,
                entry_ref: None,
                buckets: Vec::new(),
                rows: vec![crate::feature::FeatureTrimEntity {
                    external_id: 10,
                    mode: None,
                    vertices: [3, 4],
                    center_vertex: None,
                    kind: crate::feature::TrimEntityKind::Line,
                    offset: 1,
                }],
                solved_external_ids: vec![10],
                offset: 1,
            }),
            trim_vertices: None,
            order_table: None,
            section_3d: None,
            dimensions: None,
            relations: None,
            saved_section: None,
            offset: 0,
        };
        let trim_vertices = BTreeMap::from([(3, [0.0, 4.0]), (4, [7.0, 4.0])]);
        assert_eq!(
            trimmed_section_segment_geometry_with_missing_line(
                &definition,
                &BTreeMap::new(),
                &trim_vertices,
                &segment,
                None,
            ),
            Some(SketchGeometry::Line {
                start: Point2::new(0.0, 4.0),
                end: Point2::new(7.0, 4.0),
            })
        );

        let mut duplicate = definition;
        duplicate
            .segments
            .as_mut()
            .expect("segments")
            .rows
            .push(crate::feature::FeatureSegment {
                offset: 2,
                ..segment
            });
        assert_eq!(
            trimmed_section_segment_geometry_with_missing_line(
                &duplicate,
                &BTreeMap::new(),
                &trim_vertices,
                &duplicate.segments.as_ref().expect("segments").rows[0],
                None,
            ),
            None
        );
    }
}
