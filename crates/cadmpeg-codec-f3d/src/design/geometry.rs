// SPDX-License-Identifier: Apache-2.0
//! Sketch-arrangement and profile-containment computational geometry.

use crate::design::profile_select::historical_face_points;
use crate::records::{DesignExtrudeSelectionMember, SketchRelationOperand};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use std::collections::{HashMap, HashSet};

#[derive(Clone)]
struct SketchArrangementEdge {
    nodes: [usize; 2],
    boundary: cadmpeg_ir::features::SketchProfileBoundaryUse,
    polyline: Vec<Point2>,
}

pub(crate) struct SketchArrangementFace {
    boundary: Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>,
    polyline: Vec<Point2>,
}

pub(crate) fn arrangement_region_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point2],
    tolerance: f64,
) -> Option<cadmpeg_ir::features::SketchProfileRegion> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let faces = sketch_arrangement_faces(sketch, entities, tolerance)?;
    let mut boundary_matches = faces.iter().filter(|face| {
        points.iter().all(|point| {
            face.boundary
                .iter()
                .any(|use_| point_on_profile_boundary_use(*point, use_, entities, tolerance))
        })
    });
    let boundary = boundary_matches.next();
    if boundary.is_some() && boundary_matches.next().is_none() {
        return Some(SketchProfileRegion::Trimmed {
            outer_boundary: boundary?.boundary.clone(),
            hole_boundaries: Vec::new(),
        });
    }
    let mut interior_matches = faces.iter().filter(|face| {
        points.iter().all(|point| {
            !face
                .boundary
                .iter()
                .any(|use_| point_on_profile_boundary_use(*point, use_, entities, tolerance))
                && point_in_polygon(*point, &face.polyline)
        })
    });
    let interior = interior_matches.next()?;
    interior_matches
        .next()
        .is_none()
        .then(|| SketchProfileRegion::Trimmed {
            outer_boundary: interior.boundary.clone(),
            hole_boundaries: Vec::new(),
        })
}

pub(crate) fn sketch_arrangement_faces(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<Vec<SketchArrangementFace>> {
    use cadmpeg_ir::features::SketchProfileBoundaryUse;
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut nodes = Vec::<Point2>::new();
    let mut pending = Vec::<SketchProfileBoundaryUse>::new();
    let mut circles = Vec::new();
    for profile in &sketch.profiles {
        for use_ in profile {
            let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
            if matches!(entity.geometry, SketchGeometry::Circle { .. }) {
                circles.push((use_, entity));
                continue;
            }
            let range = sketch_geometry_parameter_range(&entity.geometry)?;
            for point in [
                sketch_geometry_point(&entity.geometry, range[0])?,
                sketch_geometry_point(&entity.geometry, range[1])?,
            ] {
                arrangement_node(&mut nodes, point, tolerance);
            }
            pending.push(SketchProfileBoundaryUse {
                entity: entity.id.clone(),
                parameter_range: range,
                reversed: use_.reversed,
            });
        }
    }
    for (use_, entity) in circles {
        let SketchGeometry::Circle { center, radius } = entity.geometry else {
            unreachable!("circle collection contains only circles")
        };
        let mut angles = nodes
            .iter()
            .filter(|point| (point_distance(**point, center) - radius.0).abs() <= tolerance)
            .map(|point| {
                (point.v - center.v)
                    .atan2(point.u - center.u)
                    .rem_euclid(std::f64::consts::TAU)
            })
            .collect::<Vec<_>>();
        angles.sort_by(f64::total_cmp);
        angles.dedup_by(|left, right| (*left - *right).abs() <= tolerance / radius.0);
        if angles.len() < 2 {
            return None;
        }
        for index in 0..angles.len() {
            let start = angles[index];
            let mut end = angles[(index + 1) % angles.len()];
            if index + 1 == angles.len() {
                end += std::f64::consts::TAU;
            }
            let range = [start, end];
            pending.push(SketchProfileBoundaryUse {
                entity: use_.entity.clone(),
                parameter_range: range,
                reversed: use_.reversed,
            });
        }
    }
    let mut split_pending = Vec::new();
    for boundary in pending {
        let entity = entities
            .iter()
            .find(|entity| entity.id == boundary.entity)?;
        let parameters = arrangement_split_parameters(
            &entity.geometry,
            boundary.parameter_range,
            &nodes,
            tolerance,
        )?;
        for parameters in parameters.windows(2) {
            let range = [parameters[0], parameters[1]];
            split_pending.push((
                SketchProfileBoundaryUse {
                    entity: boundary.entity.clone(),
                    parameter_range: range,
                    reversed: boundary.reversed,
                },
                profile_use_polyline(entity, range, boundary.reversed, tolerance)?,
            ));
        }
    }
    let mut edges = Vec::<SketchArrangementEdge>::new();
    for (boundary, polyline) in split_pending {
        let edge = SketchArrangementEdge {
            nodes: [
                arrangement_node(&mut nodes, *polyline.first()?, tolerance),
                arrangement_node(&mut nodes, *polyline.last()?, tolerance),
            ],
            boundary,
            polyline,
        };
        if edge.nodes[0] == edge.nodes[1] {
            return None;
        }
        if edges.iter().any(|candidate| {
            (candidate.nodes == edge.nodes || candidate.nodes == [edge.nodes[1], edge.nodes[0]])
                && arrangement_edges_coincident(candidate, &edge, entities, tolerance)
        }) {
            continue;
        }
        edges.push(edge);
    }
    if edges.len() < 3 {
        return None;
    }
    let edge_tubes = edges
        .iter()
        .map(|edge| arrangement_edge_tubes(edge, entities, tolerance))
        .collect::<Option<Vec<_>>>()?;
    let edge_bounds = edge_tubes
        .iter()
        .map(|tubes| certified_tube_bounds(tubes))
        .collect::<Option<Vec<_>>>()?;
    for left_index in 0..edges.len() {
        for right_index in left_index + 1..edges.len() {
            let left = &edges[left_index];
            let right = &edges[right_index];
            let shared_nodes = left
                .nodes
                .into_iter()
                .filter(|node| right.nodes.contains(node))
                .collect::<Vec<_>>();
            if !shared_nodes.is_empty() {
                if !arrangement_edges_meet_only_at_nodes(
                    left,
                    right,
                    entities,
                    &nodes,
                    &shared_nodes,
                    tolerance,
                ) {
                    return None;
                }
                continue;
            }
            let left_bounds = edge_bounds[left_index];
            let right_bounds = edge_bounds[right_index];
            if left_bounds[1].u < right_bounds[0].u
                || right_bounds[1].u < left_bounds[0].u
                || left_bounds[1].v < right_bounds[0].v
                || right_bounds[1].v < left_bounds[0].v
            {
                continue;
            }
            if !arrangement_edges_proven_disjoint(
                left,
                right,
                entities,
                &edge_tubes[left_index],
                &edge_tubes[right_index],
            ) {
                return None;
            }
        }
    }
    let mut outgoing = vec![Vec::<(usize, bool, f64)>::new(); nodes.len()];
    for (edge_index, edge) in edges.iter().enumerate() {
        let forward = edge.polyline.get(1)?;
        let reverse = edge.polyline.get(edge.polyline.len().checked_sub(2)?)?;
        outgoing[edge.nodes[0]].push((
            edge_index,
            false,
            (forward.v - nodes[edge.nodes[0]].v).atan2(forward.u - nodes[edge.nodes[0]].u),
        ));
        outgoing[edge.nodes[1]].push((
            edge_index,
            true,
            (reverse.v - nodes[edge.nodes[1]].v).atan2(reverse.u - nodes[edge.nodes[1]].u),
        ));
    }
    if outgoing.iter().any(|uses| uses.len() < 2) {
        return None;
    }
    for uses in &mut outgoing {
        uses.sort_by(|left, right| left.2.total_cmp(&right.2));
    }
    let mut visited = vec![[false; 2]; edges.len()];
    let mut faces = Vec::new();
    for edge_index in 0..edges.len() {
        for reversed in [false, true] {
            if visited[edge_index][usize::from(reversed)] {
                continue;
            }
            let start = (edge_index, reversed);
            let mut current = start;
            let mut boundary = Vec::new();
            let mut polyline = Vec::new();
            loop {
                let (index, reverse) = current;
                if visited[index][usize::from(reverse)] {
                    if current != start {
                        return None;
                    }
                    break;
                }
                visited[index][usize::from(reverse)] = true;
                let edge = &edges[index];
                let mut use_ = edge.boundary.clone();
                let mut points = edge.polyline.clone();
                if reverse {
                    use_.reversed = !use_.reversed;
                    points.reverse();
                }
                boundary.push(use_);
                polyline.extend(points.into_iter().take(edge.polyline.len() - 1));
                let destination = edge.nodes[usize::from(!reverse)];
                let uses = &outgoing[destination];
                let twin = uses.iter().position(|(candidate, candidate_reverse, _)| {
                    *candidate == index && *candidate_reverse != reverse
                })?;
                let next = uses[(twin + uses.len() - 1) % uses.len()];
                current = (next.0, next.1);
            }
            if polyline.len() >= 3 && signed_polygon_area(&polyline) > tolerance * tolerance {
                faces.push(SketchArrangementFace { boundary, polyline });
            }
        }
    }
    (!faces.is_empty()).then_some(faces)
}

fn arrangement_edges_meet_only_at_nodes(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    if arrangement_line_nurbs_meet_only_at_endpoint(
        left,
        right,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) || arrangement_line_nurbs_meet_only_at_endpoint(
        right,
        left,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) {
        return true;
    }
    if arrangement_arc_nurbs_meet_only_at_endpoint(
        left,
        right,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) || arrangement_arc_nurbs_meet_only_at_endpoint(
        right,
        left,
        entities,
        nodes,
        shared_nodes,
        tolerance,
    ) {
        return true;
    }
    let Some(left_segment) = arrangement_analytic_segment(left, entities) else {
        return false;
    };
    let Some(right_segment) = arrangement_analytic_segment(right, entities) else {
        return false;
    };
    if let (
        ProfileBoundarySegment::Arc {
            center: left_center,
            radius: left_radius,
            start_angle: left_start,
            end_angle: left_end,
            ..
        },
        ProfileBoundarySegment::Arc {
            center: right_center,
            radius: right_radius,
            start_angle: right_start,
            end_angle: right_end,
            ..
        },
    ) = (&left_segment, &right_segment)
    {
        if left_center == right_center && left_radius == right_radius {
            let strictly_inside = |angle: f64, start: f64, end: f64| {
                let parameter_tolerance =
                    (tolerance / (left_radius * (end - start).abs())).min(0.5);
                directed_angle_parameter(angle, start, end).is_some_and(|parameter| {
                    parameter > parameter_tolerance && parameter < 1.0 - parameter_tolerance
                })
            };
            return !strictly_inside(*left_start, *right_start, *right_end)
                && !strictly_inside(*left_end, *right_start, *right_end)
                && !strictly_inside(*right_start, *left_start, *left_end)
                && !strictly_inside(*right_end, *left_start, *left_end);
        }
    }
    analytic_segment_intersections(&left_segment, &right_segment).is_some_and(|intersections| {
        intersections.iter().all(|intersection| {
            shared_nodes
                .iter()
                .any(|node| point_distance(*intersection, nodes[*node]) <= tolerance)
        })
    })
}

fn arrangement_arc_nurbs_meet_only_at_endpoint(
    arc: &SketchArrangementEdge,
    nurbs: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    if shared_nodes.len() != 1 {
        return false;
    }
    let Some(arc_entity) = entities
        .iter()
        .find(|entity| entity.id == arc.boundary.entity)
    else {
        return false;
    };
    let Some(nurbs_entity) = entities
        .iter()
        .find(|entity| entity.id == nurbs.boundary.entity)
    else {
        return false;
    };
    let (center, radius) = match arc_entity.geometry {
        SketchGeometry::Circle { center, radius } | SketchGeometry::Arc { center, radius, .. } => {
            (center, radius.0)
        }
        _ => return false,
    };
    let SketchGeometry::Nurbs {
        control_points,
        weights,
        periodic: false,
        ..
    } = &nurbs_entity.geometry
    else {
        return false;
    };
    if weights.as_ref().is_some_and(|weights| {
        weights.len() != control_points.len() || weights.iter().any(|weight| *weight <= 0.0)
    }) || sketch_geometry_parameter_range(&nurbs_entity.geometry)
        != Some(nurbs.boundary.parameter_range)
    {
        return false;
    }
    let shared = nodes[shared_nodes[0]];
    if (point_distance(center, shared) - radius).abs() > tolerance {
        return false;
    }
    let first_shared = point_distance(control_points[0], shared) <= tolerance;
    let last_shared = control_points
        .last()
        .is_some_and(|point| point_distance(*point, shared) <= tolerance);
    if first_shared == last_shared {
        return false;
    }
    let endpoint = if first_shared {
        0
    } else {
        control_points.len() - 1
    };
    let normal = Point2::new(shared.u - center.u, shared.v - center.v);
    let support = |point: Point2| normal.u * (point.u - shared.u) + normal.v * (point.v - shared.v);
    let threshold = tolerance * radius;
    support(control_points[endpoint]).abs() <= threshold
        && (control_points
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != endpoint)
            .all(|(_, point)| support(*point) > threshold)
            || control_points
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != endpoint)
                .all(|(_, point)| point_distance(center, *point) < radius - tolerance))
}

fn arrangement_line_nurbs_meet_only_at_endpoint(
    line: &SketchArrangementEdge,
    nurbs: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    nodes: &[Point2],
    shared_nodes: &[usize],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    if shared_nodes.len() != 1 {
        return false;
    }
    let Some(line_entity) = entities
        .iter()
        .find(|entity| entity.id == line.boundary.entity)
    else {
        return false;
    };
    let Some(nurbs_entity) = entities
        .iter()
        .find(|entity| entity.id == nurbs.boundary.entity)
    else {
        return false;
    };
    let SketchGeometry::Line { start, end } = line_entity.geometry else {
        return false;
    };
    let SketchGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic: false,
    } = &nurbs_entity.geometry
    else {
        return false;
    };
    if weights.as_ref().is_some_and(|weights| {
        weights.len() != control_points.len() || weights.iter().any(|weight| *weight <= 0.0)
    }) {
        return false;
    }
    let Some(domain) = sketch_geometry_parameter_range(&nurbs_entity.geometry) else {
        return false;
    };
    if nurbs.boundary.parameter_range != domain
        || usize::try_from(*degree).ok().is_none()
        || knots.is_empty()
    {
        return false;
    }
    let shared = nodes[shared_nodes[0]];
    let first_shared = point_distance(control_points[0], shared) <= tolerance;
    let last_shared = control_points
        .last()
        .is_some_and(|point| point_distance(*point, shared) <= tolerance);
    if first_shared == last_shared {
        return false;
    }
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let line_length = point_distance(start, end);
    if line_length <= tolerance {
        return false;
    }
    let side =
        |point: Point2| direction.u * (point.v - start.v) - direction.v * (point.u - start.u);
    let endpoint = if first_shared {
        0
    } else {
        control_points.len() - 1
    };
    let threshold = tolerance * line_length;
    if side(control_points[endpoint]).abs() > threshold {
        return false;
    }
    let mut signs = control_points
        .iter()
        .enumerate()
        .filter(|(index, _)| *index != endpoint)
        .map(|(_, point)| side(*point));
    let Some(first_side) = signs.next() else {
        return false;
    };
    first_side.abs() > threshold
        && signs.all(|candidate| {
            candidate.abs() > threshold
                && candidate.is_sign_positive() == first_side.is_sign_positive()
        })
}

pub(crate) fn analytic_segment_intersections(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> Option<Vec<Point2>> {
    match (left, right) {
        (
            ProfileBoundarySegment::Line { start: a, end: b },
            ProfileBoundarySegment::Line { start: c, end: d },
        ) => {
            let ab = Point2::new(b.u - a.u, b.v - a.v);
            let cd = Point2::new(d.u - c.u, d.v - c.v);
            let denominator = ab.u * cd.v - ab.v * cd.u;
            if denominator == 0.0 {
                return None;
            }
            let ac = Point2::new(c.u - a.u, c.v - a.v);
            let parameter = (ac.u * cd.v - ac.v * cd.u) / denominator;
            let other_parameter = (ac.u * ab.v - ac.v * ab.u) / denominator;
            Some(
                ((0.0..=1.0).contains(&parameter) && (0.0..=1.0).contains(&other_parameter))
                    .then(|| Point2::new(a.u + parameter * ab.u, a.v + parameter * ab.v))
                    .into_iter()
                    .collect(),
            )
        }
        (ProfileBoundarySegment::Line { start, end }, arc @ ProfileBoundarySegment::Arc { .. })
        | (arc @ ProfileBoundarySegment::Arc { .. }, ProfileBoundarySegment::Line { start, end }) => {
            line_arc_intersection_points((*start, *end), arc)
        }
        (left @ ProfileBoundarySegment::Arc { .. }, right @ ProfileBoundarySegment::Arc { .. }) => {
            arc_intersection_points(left, right)
        }
    }
}

fn line_arc_intersection_points(
    (start, end): (Point2, Point2),
    arc: &ProfileBoundarySegment,
) -> Option<Vec<Point2>> {
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = arc
    else {
        return None;
    };
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let offset = Point2::new(start.u - center.u, start.v - center.v);
    let quadratic = direction.u * direction.u + direction.v * direction.v;
    if quadratic == 0.0 {
        return Some(Vec::new());
    }
    let linear = 2.0 * (offset.u * direction.u + offset.v * direction.v);
    let constant = offset.u * offset.u + offset.v * offset.v - radius * radius;
    let discriminant = linear * linear - 4.0 * quadratic * constant;
    let error =
        64.0 * f64::EPSILON * (linear * linear + (4.0 * quadratic * constant).abs()).max(1.0);
    if discriminant < -error {
        return Some(Vec::new());
    }
    let root = discriminant.max(0.0).sqrt();
    let mut points = Vec::new();
    for signed_root in [-root, root] {
        let parameter = (-linear + signed_root) / (2.0 * quadratic);
        if (0.0..=1.0).contains(&parameter) {
            let point = Point2::new(
                start.u + parameter * direction.u,
                start.v + parameter * direction.v,
            );
            if directed_angle_parameter(
                (point.v - center.v).atan2(point.u - center.u),
                *start_angle,
                *end_angle,
            )
            .is_some()
                && !points.contains(&point)
            {
                points.push(point);
            }
        }
    }
    Some(points)
}

fn arc_intersection_points(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> Option<Vec<Point2>> {
    let ProfileBoundarySegment::Arc {
        center: lc,
        radius: lr,
        start_angle: ls,
        end_angle: le,
    } = left
    else {
        return None;
    };
    let ProfileBoundarySegment::Arc {
        center: rc,
        radius: rr,
        start_angle: rs,
        end_angle: re,
    } = right
    else {
        return None;
    };
    let du = rc.u - lc.u;
    let dv = rc.v - lc.v;
    let distance_squared = du * du + dv * dv;
    if distance_squared == 0.0 {
        return (*lr != *rr).then(Vec::new);
    }
    let distance = distance_squared.sqrt();
    if distance > lr + rr || distance < (lr - rr).abs() {
        return Some(Vec::new());
    }
    let along = (lr * lr - rr * rr + distance_squared) / (2.0 * distance);
    let height_squared = lr * lr - along * along;
    let error = 64.0 * f64::EPSILON * (lr * lr + along * along).max(1.0);
    if height_squared < -error {
        return Some(Vec::new());
    }
    let base = Point2::new(lc.u + along * du / distance, lc.v + along * dv / distance);
    let height = height_squared.max(0.0).sqrt();
    let mut points = Vec::new();
    for signed_height in [height, -height] {
        let point = Point2::new(
            base.u - signed_height * dv / distance,
            base.v + signed_height * du / distance,
        );
        if directed_angle_parameter((point.v - lc.v).atan2(point.u - lc.u), *ls, *le).is_some()
            && directed_angle_parameter((point.v - rc.v).atan2(point.u - rc.u), *rs, *re).is_some()
            && !points.contains(&point)
        {
            points.push(point);
        }
    }
    Some(points)
}

fn arrangement_split_parameters(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    range: [f64; 2],
    nodes: &[Point2],
    tolerance: f64,
) -> Option<Vec<f64>> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut parameters = vec![range[0], range[1]];
    match geometry {
        SketchGeometry::Line { start, end } => {
            let direction = Point2::new(end.u - start.u, end.v - start.v);
            let length_squared = direction.u * direction.u + direction.v * direction.v;
            if length_squared <= tolerance * tolerance {
                return None;
            }
            for point in nodes {
                let parameter = ((point.u - start.u) * direction.u
                    + (point.v - start.v) * direction.v)
                    / length_squared;
                if parameter > 0.0
                    && parameter < 1.0
                    && point_segment_distance(*point, (*start, *end)) <= tolerance
                {
                    parameters.push(parameter);
                }
            }
        }
        SketchGeometry::Arc { center, radius, .. } => {
            for point in nodes {
                if (point_distance(*point, *center) - radius.0).abs() > tolerance {
                    continue;
                }
                let angle = (point.v - center.v).atan2(point.u - center.u);
                if let Some(parameter) = directed_angle_parameter(angle, range[0], range[1]) {
                    if parameter > 0.0 && parameter < 1.0 {
                        parameters.push(range[0] + parameter * (range[1] - range[0]));
                    }
                }
            }
        }
        _ => {}
    }
    parameters.sort_by(|left, right| {
        ((left - range[0]) / (range[1] - range[0]))
            .total_cmp(&((right - range[0]) / (range[1] - range[0])))
    });
    let parameter_tolerance =
        tolerance / sketch_geometry_speed_bound(geometry, range)?.max(tolerance);
    parameters.dedup_by(|left, right| (*left - *right).abs() <= parameter_tolerance);
    (parameters.len() >= 2).then_some(parameters)
}

fn arrangement_node(nodes: &mut Vec<Point2>, point: Point2, tolerance: f64) -> usize {
    nodes
        .iter()
        .position(|candidate| point_distance(*candidate, point) <= tolerance)
        .unwrap_or_else(|| {
            nodes.push(point);
            nodes.len() - 1
        })
}

fn arrangement_edges_coincident(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    if left.boundary.entity == right.boundary.entity
        && left.boundary.parameter_range == right.boundary.parameter_range
    {
        return true;
    }
    let Some(left_entity) = entities
        .iter()
        .find(|entity| entity.id == left.boundary.entity)
    else {
        return false;
    };
    let Some(right_entity) = entities
        .iter()
        .find(|entity| entity.id == right.boundary.entity)
    else {
        return false;
    };
    match (&left_entity.geometry, &right_entity.geometry) {
        (SketchGeometry::Line { .. }, SketchGeometry::Line { .. }) => {
            let left_midpoint = sketch_geometry_point(
                &left_entity.geometry,
                (left.boundary.parameter_range[0] + left.boundary.parameter_range[1]) * 0.5,
            );
            left_midpoint
                .zip(right.polyline.last().copied())
                .is_some_and(|(point, end)| {
                    point_segment_distance(point, (right.polyline[0], end)) <= tolerance
                })
        }
        (
            SketchGeometry::Circle {
                center: left_center,
                radius: left_radius,
            }
            | SketchGeometry::Arc {
                center: left_center,
                radius: left_radius,
                ..
            },
            SketchGeometry::Circle {
                center: right_center,
                radius: right_radius,
            }
            | SketchGeometry::Arc {
                center: right_center,
                radius: right_radius,
                ..
            },
        ) => {
            point_distance(*left_center, *right_center) <= tolerance
                && (left_radius.0 - right_radius.0).abs() <= tolerance
                && ((left.boundary.parameter_range[1] - left.boundary.parameter_range[0]).abs()
                    - (right.boundary.parameter_range[1] - right.boundary.parameter_range[0]).abs())
                .abs()
                    <= tolerance / left_radius.0
                && sketch_geometry_point(
                    &left_entity.geometry,
                    (left.boundary.parameter_range[0] + left.boundary.parameter_range[1]) * 0.5,
                )
                .zip(sketch_geometry_point(
                    &right_entity.geometry,
                    (right.boundary.parameter_range[0] + right.boundary.parameter_range[1]) * 0.5,
                ))
                .is_some_and(|(left, right)| point_distance(left, right) <= tolerance)
        }
        _ => false,
    }
}

fn arrangement_edges_proven_disjoint(
    left: &SketchArrangementEdge,
    right: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    left_tubes: &[CertifiedCurveTube],
    right_tubes: &[CertifiedCurveTube],
) -> bool {
    match (
        arrangement_analytic_segment(left, entities),
        arrangement_analytic_segment(right, entities),
    ) {
        (Some(left), Some(right)) => !boundary_segments_intersect(&left, &right),
        _ => left_tubes.iter().all(|left| {
            right_tubes.iter().all(|right| {
                segment_distance((left.start, left.end), (right.start, right.end))
                    > left.error + right.error
            })
        }),
    }
}

fn certified_tube_bounds(tubes: &[CertifiedCurveTube]) -> Option<[Point2; 2]> {
    let first = tubes.first()?;
    Some(tubes.iter().fold(
        [
            Point2::new(
                first.start.u.min(first.end.u) - first.error,
                first.start.v.min(first.end.v) - first.error,
            ),
            Point2::new(
                first.start.u.max(first.end.u) + first.error,
                first.start.v.max(first.end.v) + first.error,
            ),
        ],
        |mut bounds, tube| {
            bounds[0].u = bounds[0].u.min(tube.start.u.min(tube.end.u) - tube.error);
            bounds[0].v = bounds[0].v.min(tube.start.v.min(tube.end.v) - tube.error);
            bounds[1].u = bounds[1].u.max(tube.start.u.max(tube.end.u) + tube.error);
            bounds[1].v = bounds[1].v.max(tube.start.v.max(tube.end.v) + tube.error);
            bounds
        },
    ))
}

fn arrangement_edge_tubes(
    edge: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<Vec<CertifiedCurveTube>> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let entity = entities
        .iter()
        .find(|entity| entity.id == edge.boundary.entity)?;
    let scale = edge
        .polyline
        .iter()
        .flat_map(|point| [point.u.abs(), point.v.abs()])
        .fold(1.0_f64, f64::max);
    let target_error = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    match &entity.geometry {
        SketchGeometry::Line { .. } => Some(vec![CertifiedCurveTube {
            start: edge.polyline[0],
            end: *edge.polyline.last()?,
            error: 0.0,
        }]),
        SketchGeometry::Circle { center, radius } | SketchGeometry::Arc { center, radius, .. } => {
            certified_arc_tubes(
                *center,
                radius.0,
                edge.boundary.parameter_range[0],
                edge.boundary.parameter_range[1],
                target_error,
            )
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } if sketch_geometry_parameter_range(&entity.geometry)
            == Some(edge.boundary.parameter_range) =>
        {
            certified_nurbs_tubes(
                *degree,
                knots,
                control_points,
                weights.as_deref(),
                target_error,
            )
        }
        _ => None,
    }
}

fn arrangement_analytic_segment(
    edge: &SketchArrangementEdge,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
) -> Option<ProfileBoundarySegment> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let entity = entities
        .iter()
        .find(|entity| entity.id == edge.boundary.entity)?;
    match &entity.geometry {
        SketchGeometry::Line { .. } => Some(ProfileBoundarySegment::Line {
            start: edge.polyline[0],
            end: *edge.polyline.last()?,
        }),
        SketchGeometry::Circle { center, radius } | SketchGeometry::Arc { center, radius, .. } => {
            Some(ProfileBoundarySegment::Arc {
                center: *center,
                radius: radius.0,
                start_angle: edge.boundary.parameter_range[0],
                end_angle: edge.boundary.parameter_range[1],
            })
        }
        _ => None,
    }
}

fn sketch_geometry_parameter_range(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
) -> Option<[f64; 2]> {
    use cadmpeg_ir::sketches::SketchGeometry;

    match geometry {
        SketchGeometry::Line { .. } => Some([0.0, 1.0]),
        SketchGeometry::Arc {
            start_angle,
            end_angle,
            ..
        } => Some([start_angle.0, end_angle.0]),
        SketchGeometry::Ellipse {
            start_angle: Some(start),
            end_angle: Some(end),
            ..
        } => Some([start.0, end.0]),
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            periodic: false,
            ..
        } => Some([
            *knots.get(usize::try_from(*degree).ok()?)?,
            *knots.get(control_points.len())?,
        ]),
        _ => None,
    }
}

fn profile_use_polyline(
    entity: &cadmpeg_ir::sketches::SketchEntity,
    range: [f64; 2],
    reversed: bool,
    tolerance: f64,
) -> Option<Vec<Point2>> {
    let travel =
        sketch_geometry_speed_bound(&entity.geometry, range)? * (range[1] - range[0]).abs();
    let scale = [
        sketch_geometry_point(&entity.geometry, range[0])?,
        sketch_geometry_point(&entity.geometry, range[1])?,
        sketch_geometry_point(&entity.geometry, (range[0] + range[1]) * 0.5)?,
    ]
    .into_iter()
    .flat_map(|point| [point.u.abs(), point.v.abs()])
    .fold(1.0_f64, f64::max);
    let target = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    // This bounded polyline supplies tangent order, winding sign, and
    // intersection witnesses only. Exact output retains source parameter
    // intervals rather than this derived representation.
    let count = (travel / target).ceil().clamp(2.0, 256.0) as usize;
    let mut points = (0..=count)
        .map(|index| {
            let parameter = range[0] + (range[1] - range[0]) * index as f64 / count as f64;
            sketch_geometry_point(&entity.geometry, parameter)
        })
        .collect::<Option<Vec<_>>>()?;
    if reversed {
        points.reverse();
    }
    Some(points)
}

fn sketch_geometry_speed_bound(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    range: [f64; 2],
) -> Option<f64> {
    use cadmpeg_ir::sketches::SketchGeometry;

    match geometry {
        SketchGeometry::Line { start, end } => Some(point_distance(*start, *end)),
        SketchGeometry::Circle { radius, .. } | SketchGeometry::Arc { radius, .. } => {
            Some(radius.0)
        }
        SketchGeometry::Ellipse {
            major_radius,
            minor_radius,
            ..
        } => Some(major_radius.0.max(minor_radius.0)),
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => nurbs_speed_bound(*degree, knots, control_points, weights.as_deref()),
        _ if range[0] == range[1] => None,
        _ => None,
    }
}

fn sketch_geometry_point(
    geometry: &cadmpeg_ir::sketches::SketchGeometry,
    parameter: f64,
) -> Option<Point2> {
    use cadmpeg_ir::sketches::SketchGeometry;

    match geometry {
        SketchGeometry::Line { start, end } => Some(Point2::new(
            start.u + parameter * (end.u - start.u),
            start.v + parameter * (end.v - start.v),
        )),
        SketchGeometry::Circle { center, radius } | SketchGeometry::Arc { center, radius, .. } => {
            Some(Point2::new(
                center.u + radius.0 * parameter.cos(),
                center.v + radius.0 * parameter.sin(),
            ))
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            ..
        } => {
            let (axis_sine, axis_cosine) = major_angle.0.sin_cos();
            Some(Point2::new(
                center.u + major_radius.0 * parameter.cos() * axis_cosine
                    - minor_radius.0 * parameter.sin() * axis_sine,
                center.v
                    + major_radius.0 * parameter.cos() * axis_sine
                    + minor_radius.0 * parameter.sin() * axis_cosine,
            ))
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => cadmpeg_ir::eval::nurbs_pcurve_uv(
            *degree,
            knots,
            control_points,
            weights.as_deref(),
            parameter,
        ),
        _ => None,
    }
}

fn point_on_profile_boundary_use(
    point: Point2,
    use_: &cadmpeg_ir::features::SketchProfileBoundaryUse,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    let Some(entity) = entities.iter().find(|entity| entity.id == use_.entity) else {
        return false;
    };
    if !point_on_sketch_entity(point, entity, tolerance) {
        return false;
    }
    match &entity.geometry {
        SketchGeometry::Circle { center, radius } | SketchGeometry::Arc { center, radius, .. } => {
            let angle = (point.v - center.v).atan2(point.u - center.u);
            directed_angle_parameter(angle, use_.parameter_range[0], use_.parameter_range[1])
                .is_some_and(|parameter| {
                    parameter >= -tolerance / radius.0 && parameter <= 1.0 + tolerance / radius.0
                })
        }
        _ => true,
    }
}

fn signed_polygon_area(vertices: &[Point2]) -> f64 {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
        .map(|(start, end)| start.u * end.v - end.u * start.v)
        .sum::<f64>()
        * 0.5
}

pub(crate) fn region_containing_points(
    sketch: &cadmpeg_ir::sketches::Sketch,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    points: &[Point3],
    tolerance: f64,
) -> Option<cadmpeg_ir::features::SketchProfileRegion> {
    use cadmpeg_ir::features::SketchProfileRegion;

    let boundaries = sketch
        .profiles
        .iter()
        .map(|profile| profile_boundary(profile, entities, tolerance))
        .collect::<Option<Vec<_>>>()?;
    let containment = boundaries
        .iter()
        .enumerate()
        .map(|(outer_index, outer)| {
            boundaries
                .iter()
                .enumerate()
                .map(|(inner_index, inner)| {
                    outer_index != inner_index && outer.strictly_contains(inner)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let projected = points
        .iter()
        .map(|point| project_to_sketch(sketch, *point))
        .collect::<Option<Vec<_>>>()?;
    let incidences = projected
        .iter()
        .map(|point| {
            sketch
                .profiles
                .iter()
                .enumerate()
                .filter(|(_, profile)| {
                    profile.iter().any(|use_| {
                        entities
                            .iter()
                            .find(|entity| entity.id == use_.entity)
                            .is_some_and(|entity| point_on_sketch_entity(*point, entity, tolerance))
                    })
                })
                .map(|(index, _)| index)
                .collect::<HashSet<_>>()
        })
        .collect::<Vec<_>>();
    let region = |outer: usize| {
        let holes = immediate_containment_children(outer, &containment);
        projected
            .iter()
            .zip(&incidences)
            .all(|(point, incident)| {
                incident.contains(&outer)
                    || holes.iter().any(|hole| incident.contains(hole))
                    || incident.is_empty()
                        && boundaries[outer].contains_point(*point)
                        && holes
                            .iter()
                            .all(|hole| !boundaries[*hole].contains_point(*point))
            })
            .then_some((outer, holes))
    };
    let closure_matches = (0..boundaries.len()).filter_map(region).collect::<Vec<_>>();
    if let [(outer, holes)] = closure_matches.as_slice() {
        return Some(SketchProfileRegion::Loops {
            outer: u32::try_from(*outer).ok()?,
            holes: holes
                .iter()
                .map(|hole| u32::try_from(*hole).ok())
                .collect::<Option<Vec<_>>>()?,
        });
    }
    if projected.iter().any(|point| {
        sketch.profiles.iter().any(|profile| {
            profile.iter().any(|use_| {
                entities
                    .iter()
                    .find(|entity| entity.id == use_.entity)
                    .is_some_and(|entity| point_on_sketch_entity(*point, entity, tolerance))
            })
        })
    }) {
        return None;
    }
    let containing = boundaries
        .iter()
        .enumerate()
        .filter(|(_, boundary)| {
            projected
                .iter()
                .all(|point| boundary.contains_point(*point))
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if containing.iter().enumerate().any(|(left_index, left)| {
        containing
            .iter()
            .skip(left_index + 1)
            .any(|right| !containment[*left][*right] && !containment[*right][*left])
    }) {
        return None;
    }
    let &outer = containing.iter().find(|candidate| {
        containing
            .iter()
            .all(|other| other == *candidate || containment[*other][**candidate])
    })?;
    let holes = immediate_containment_children(outer, &containment)
        .into_iter()
        .map(|candidate| u32::try_from(candidate).ok())
        .collect::<Option<Vec<_>>>()?;
    Some(SketchProfileRegion::Loops {
        outer: u32::try_from(outer).ok()?,
        holes,
    })
}

fn immediate_containment_children(outer: usize, containment: &[Vec<bool>]) -> Vec<usize> {
    (0..containment.len())
        .filter(|candidate| {
            *candidate != outer
                && containment[outer][*candidate]
                && !(0..containment.len()).any(|intermediate| {
                    intermediate != outer
                        && intermediate != *candidate
                        && containment[outer][intermediate]
                        && containment[intermediate][*candidate]
                })
        })
        .collect()
}

pub(crate) enum ProfileBoundary {
    Polygon(Vec<Point2>),
    CircularArcLoop(Vec<ProfileBoundarySegment>),
    Circle { center: Point2, radius: f64 },
    CertifiedLoop(CertifiedProfileLoop),
}

#[derive(Clone)]
pub(crate) struct CertifiedProfileLoop {
    vertices: Vec<Point2>,
    tubes: Vec<CertifiedCurveTube>,
}

#[derive(Clone)]
struct CertifiedCurveTube {
    start: Point2,
    end: Point2,
    error: f64,
}

pub(crate) enum ProfileBoundarySegment {
    Line {
        start: Point2,
        end: Point2,
    },
    Arc {
        center: Point2,
        radius: f64,
        start_angle: f64,
        end_angle: f64,
    },
}

impl ProfileBoundary {
    pub(crate) fn contains_point(&self, point: Point2) -> bool {
        match self {
            Self::Polygon(vertices) => point_in_polygon(point, vertices),
            Self::CircularArcLoop(segments) => point_in_circular_arc_loop(point, segments),
            Self::Circle { center, radius } => point_distance(*center, point) < *radius,
            Self::CertifiedLoop(loop_) => loop_.contains_point(point),
        }
    }

    pub(crate) fn strictly_contains(&self, inner: &Self) -> bool {
        match (self, inner) {
            (Self::Polygon(outer), Self::Polygon(inner)) => polygon_strictly_contains(outer, inner),
            (
                Self::Circle {
                    center: outer_center,
                    radius: outer_radius,
                },
                Self::Circle {
                    center: inner_center,
                    radius: inner_radius,
                },
            ) => point_distance(*outer_center, *inner_center) + inner_radius < *outer_radius,
            (Self::Polygon(outer), Self::Circle { center, radius }) => {
                point_in_polygon(*center, outer)
                    && polygon_edges(outer)
                        .all(|edge| point_segment_distance(*center, edge) > *radius)
            }
            (Self::CircularArcLoop(outer), Self::Circle { center, radius }) => {
                point_in_circular_arc_loop(*center, outer)
                    && outer
                        .iter()
                        .all(|segment| point_boundary_segment_distance(*center, segment) > *radius)
            }
            (Self::Circle { center, radius }, Self::Polygon(inner)) => inner
                .iter()
                .all(|point| point_distance(*center, *point) < *radius),
            (Self::Circle { center, radius }, Self::CircularArcLoop(inner)) => inner
                .iter()
                .all(|segment| boundary_segment_max_distance(*center, segment) < *radius),
            (Self::Polygon(outer), Self::CircularArcLoop(inner)) => {
                !polygon_arc_loop_intersects(outer, inner)
                    && inner.first().is_some_and(|segment| {
                        point_in_polygon(boundary_segment_endpoints(segment).0, outer)
                    })
            }
            (Self::CircularArcLoop(outer), Self::Polygon(inner)) => {
                !polygon_arc_loop_intersects(inner, outer)
                    && inner
                        .first()
                        .is_some_and(|point| point_in_circular_arc_loop(*point, outer))
            }
            (Self::CircularArcLoop(outer), Self::CircularArcLoop(inner)) => {
                !arc_loops_intersect(outer, inner)
                    && inner.first().is_some_and(|segment| {
                        point_in_circular_arc_loop(boundary_segment_endpoints(segment).0, outer)
                    })
            }
            (outer, inner) => outer
                .certified_loop()
                .zip(inner.certified_loop())
                .is_some_and(|(outer, inner)| outer.strictly_contains(&inner)),
        }
    }

    fn certified_loop(&self) -> Option<CertifiedProfileLoop> {
        match self {
            Self::Polygon(vertices) => CertifiedProfileLoop::from_vertices(vertices.clone()),
            Self::CircularArcLoop(segments) => certified_analytic_loop(segments),
            Self::Circle { center, radius } => certified_circle(*center, *radius),
            Self::CertifiedLoop(loop_) => Some(loop_.clone()),
        }
    }
}

impl CertifiedProfileLoop {
    fn from_vertices(vertices: Vec<Point2>) -> Option<Self> {
        (vertices.len() >= 3).then(|| Self {
            tubes: polygon_edges(&vertices)
                .map(|(start, end)| CertifiedCurveTube {
                    start,
                    end,
                    error: 0.0,
                })
                .collect(),
            vertices,
        })
    }

    fn contains_point(&self, point: Point2) -> bool {
        self.tubes
            .iter()
            .all(|tube| point_segment_distance(point, (tube.start, tube.end)) > tube.error)
            && point_in_polygon(point, &self.vertices)
    }

    fn strictly_contains(&self, inner: &Self) -> bool {
        self.tubes.iter().all(|outer| {
            inner.tubes.iter().all(|inner| {
                segment_distance((outer.start, outer.end), (inner.start, inner.end))
                    > outer.error + inner.error
            })
        }) && inner
            .vertices
            .first()
            .is_some_and(|point| self.contains_point(*point))
    }
}

fn profile_boundary(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<ProfileBoundary> {
    use cadmpeg_ir::sketches::SketchGeometry;

    if let [use_] = profile {
        let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
        if let SketchGeometry::Circle { center, radius } = entity.geometry {
            return Some(ProfileBoundary::Circle {
                center,
                radius: radius.0,
            });
        }
    }
    line_profile_vertices(profile, entities, tolerance)
        .map(ProfileBoundary::Polygon)
        .or_else(|| {
            circular_arc_profile_segments(profile, entities, tolerance)
                .map(ProfileBoundary::CircularArcLoop)
        })
        .or_else(|| {
            certified_profile_loop(profile, entities, tolerance).map(ProfileBoundary::CertifiedLoop)
        })
}

fn certified_profile_loop(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<CertifiedProfileLoop> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let scale = entities
        .iter()
        .filter_map(sketch_entity_endpoints)
        .flatten()
        .flat_map(|point| [point.u.abs(), point.v.abs()])
        .fold(1.0_f64, f64::max);
    // The tube need only separate the selected point and peer boundaries; it
    // is not a geometric approximation exposed by the codec.  A square-root
    // scale keeps the conservative tube practical while exact boundary tests
    // continue to govern the source linear tolerance.
    let target_error = (tolerance * scale).sqrt().max(64.0 * f64::EPSILON * scale);
    let mut vertices = Vec::new();
    let mut tubes = Vec::new();
    let mut previous_end = None;
    for use_ in profile {
        let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
        let mut entity_tubes = match &entity.geometry {
            SketchGeometry::Line { start, end } => vec![CertifiedCurveTube {
                start: *start,
                end: *end,
                error: 0.0,
            }],
            SketchGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => certified_arc_tubes(*center, radius.0, start_angle.0, end_angle.0, target_error)?,
            SketchGeometry::Nurbs {
                degree,
                knots,
                control_points,
                weights,
                periodic: false,
            } => certified_nurbs_tubes(
                *degree,
                knots,
                control_points,
                weights.as_deref(),
                target_error,
            )?,
            _ => return None,
        };
        if use_.reversed {
            entity_tubes.reverse();
            for tube in &mut entity_tubes {
                std::mem::swap(&mut tube.start, &mut tube.end);
            }
        }
        let first = entity_tubes.first()?.start;
        if previous_end.is_some_and(|end| point_distance(end, first) > tolerance) {
            return None;
        }
        vertices.extend(entity_tubes.iter().map(|tube| tube.start));
        previous_end = entity_tubes.last().map(|tube| tube.end);
        tubes.extend(entity_tubes);
    }
    if tubes.len() < 3
        || previous_end.is_none_or(|end| point_distance(end, tubes[0].start) > tolerance)
    {
        return None;
    }
    Some(CertifiedProfileLoop { vertices, tubes })
}

fn certified_analytic_loop(segments: &[ProfileBoundarySegment]) -> Option<CertifiedProfileLoop> {
    let scale = segments
        .iter()
        .flat_map(|segment| {
            let (start, end) = boundary_segment_endpoints(segment);
            [start.u.abs(), start.v.abs(), end.u.abs(), end.v.abs()]
        })
        .fold(1.0_f64, f64::max);
    let tolerance = 1.0e-6 * scale;
    let mut vertices = Vec::new();
    let mut tubes = Vec::new();
    for segment in segments {
        let segment_tubes = match segment {
            ProfileBoundarySegment::Line { start, end } => vec![CertifiedCurveTube {
                start: *start,
                end: *end,
                error: 0.0,
            }],
            ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => certified_arc_tubes(*center, *radius, *start_angle, *end_angle, tolerance)?,
        };
        vertices.extend(segment_tubes.iter().map(|tube| tube.start));
        tubes.extend(segment_tubes);
    }
    Some(CertifiedProfileLoop { vertices, tubes })
}

fn certified_circle(center: Point2, radius: f64) -> Option<CertifiedProfileLoop> {
    let tolerance = 1.0e-6 * (1.0 + center.u.abs().max(center.v.abs()).max(radius));
    let tubes = certified_arc_tubes(center, radius, 0.0, std::f64::consts::TAU, tolerance)?;
    let vertices = tubes.iter().map(|tube| tube.start).collect();
    Some(CertifiedProfileLoop { vertices, tubes })
}

fn certified_arc_tubes(
    center: Point2,
    radius: f64,
    start: f64,
    end: f64,
    target_error: f64,
) -> Option<Vec<CertifiedCurveTube>> {
    let sweep = end - start;
    if !radius.is_finite() || radius <= 0.0 || !sweep.is_finite() || sweep == 0.0 {
        return None;
    }
    let count = subdivision_count(radius * sweep.abs(), target_error)?;
    let error = radius * sweep.abs() / count as f64;
    (0..count)
        .map(|index| {
            let parameter = |ordinal: usize| start + sweep * ordinal as f64 / count as f64;
            let point = |angle: f64| {
                Point2::new(
                    center.u + radius * angle.cos(),
                    center.v + radius * angle.sin(),
                )
            };
            Some(CertifiedCurveTube {
                start: point(parameter(index)),
                end: point(parameter(index + 1)),
                error,
            })
        })
        .collect()
}

fn certified_nurbs_tubes(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
    target_error: f64,
) -> Option<Vec<CertifiedCurveTube>> {
    let speed = nurbs_speed_bound(degree, knots, control_points, weights)?;
    let degree = usize::try_from(degree).ok()?;
    let count = control_points.len();
    let mut tubes = Vec::new();
    for span in knots.get(degree..=count)?.windows(2) {
        if span[0] == span[1] {
            continue;
        }
        let subdivisions = subdivision_count(speed * (span[1] - span[0]), target_error)?;
        let error = speed * (span[1] - span[0]) / subdivisions as f64;
        for index in 0..subdivisions {
            let parameter = |ordinal: usize| {
                span[0] + (span[1] - span[0]) * ordinal as f64 / subdivisions as f64
            };
            tubes.push(CertifiedCurveTube {
                start: cadmpeg_ir::eval::nurbs_pcurve_uv(
                    degree as u32,
                    knots,
                    control_points,
                    weights,
                    parameter(index),
                )?,
                end: cadmpeg_ir::eval::nurbs_pcurve_uv(
                    degree as u32,
                    knots,
                    control_points,
                    weights,
                    parameter(index + 1),
                )?,
                error,
            });
        }
    }
    (!tubes.is_empty()).then_some(tubes)
}

fn subdivision_count(travel_bound: f64, target_error: f64) -> Option<usize> {
    const MAX_SUBDIVISIONS: usize = 100_000;
    if !travel_bound.is_finite() || travel_bound < 0.0 || !target_error.is_finite() {
        return None;
    }
    let count = (travel_bound / target_error).ceil().max(1.0);
    (count <= MAX_SUBDIVISIONS as f64).then_some(count as usize)
}

fn nurbs_speed_bound(
    degree: u32,
    knots: &[f64],
    control_points: &[Point2],
    weights: Option<&[f64]>,
) -> Option<f64> {
    let degree_usize = usize::try_from(degree).ok()?;
    let count = control_points.len();
    if degree_usize == 0
        || count <= degree_usize
        || knots.len() < count.checked_add(degree_usize)?.checked_add(1)?
    {
        return None;
    }
    let owned_weights;
    let weights = match weights {
        Some(weights) if weights.len() == count => weights,
        Some(_) => return None,
        None => {
            owned_weights = vec![1.0; count];
            &owned_weights
        }
    };
    if knots.iter().any(|value| !value.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
        || control_points.iter().zip(weights).any(|(point, weight)| {
            !point.u.is_finite() || !point.v.is_finite() || !weight.is_finite() || *weight <= 0.0
        })
    {
        return None;
    }
    let minimum_weight = weights.iter().copied().fold(f64::INFINITY, f64::min);
    let maximum_numerator = control_points
        .iter()
        .zip(weights)
        .map(|(point, weight)| weight * point.u.hypot(point.v))
        .fold(0.0_f64, f64::max);
    let mut numerator_speed = 0.0_f64;
    let mut weight_speed = 0.0_f64;
    for index in 0..count - 1 {
        let denominator = knots[index + degree_usize + 1] - knots[index + 1];
        if denominator == 0.0 {
            continue;
        }
        let factor = f64::from(degree) / denominator;
        let first = Point2::new(
            weights[index] * control_points[index].u,
            weights[index] * control_points[index].v,
        );
        let second = Point2::new(
            weights[index + 1] * control_points[index + 1].u,
            weights[index + 1] * control_points[index + 1].v,
        );
        numerator_speed =
            numerator_speed.max(factor * (second.u - first.u).hypot(second.v - first.v));
        weight_speed = weight_speed.max(factor * (weights[index + 1] - weights[index]).abs());
    }
    let bound = numerator_speed / minimum_weight
        + maximum_numerator * weight_speed / minimum_weight.powi(2);
    bound.is_finite().then_some(bound)
}

fn circular_arc_profile_segments(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<Vec<ProfileBoundarySegment>> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut segments = Vec::with_capacity(profile.len());
    let mut previous_end = None;
    for use_ in profile {
        let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
        let segment = match entity.geometry {
            SketchGeometry::Line { start, end } => {
                let [start, end] = if use_.reversed {
                    [end, start]
                } else {
                    [start, end]
                };
                ProfileBoundarySegment::Line { start, end }
            }
            SketchGeometry::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => {
                let (start_angle, end_angle) = if use_.reversed {
                    (end_angle.0, start_angle.0)
                } else {
                    (start_angle.0, end_angle.0)
                };
                ProfileBoundarySegment::Arc {
                    center,
                    radius: radius.0,
                    start_angle,
                    end_angle,
                }
            }
            _ => return None,
        };
        let (start, end) = boundary_segment_endpoints(&segment);
        if previous_end.is_some_and(|previous| point_distance(previous, start) > tolerance) {
            return None;
        }
        previous_end = Some(end);
        segments.push(segment);
    }
    let first_start = segments.first().map(boundary_segment_endpoints)?.0;
    (segments.len() >= 2
        && previous_end.is_some_and(|end| point_distance(end, first_start) <= tolerance))
    .then_some(segments)
}

fn line_profile_vertices(
    profile: &[cadmpeg_ir::sketches::SketchEntityUse],
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    tolerance: f64,
) -> Option<Vec<Point2>> {
    use cadmpeg_ir::sketches::SketchGeometry;

    let mut vertices = Vec::with_capacity(profile.len());
    let mut previous_end = None;
    for use_ in profile {
        let entity = entities.iter().find(|entity| entity.id == use_.entity)?;
        let SketchGeometry::Line { start, end } = entity.geometry else {
            return None;
        };
        let [start, end] = if use_.reversed {
            [end, start]
        } else {
            [start, end]
        };
        if previous_end.is_some_and(|previous| point_distance(previous, start) > tolerance) {
            return None;
        }
        vertices.push(start);
        previous_end = Some(end);
    }
    if vertices.len() < 3
        || previous_end.is_none_or(|end| point_distance(end, vertices[0]) > tolerance)
    {
        return None;
    }
    Some(vertices)
}

fn boundary_segment_endpoints(segment: &ProfileBoundarySegment) -> (Point2, Point2) {
    match segment {
        ProfileBoundarySegment::Line { start, end } => (*start, *end),
        ProfileBoundarySegment::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => (
            Point2::new(
                center.u + radius * start_angle.cos(),
                center.v + radius * start_angle.sin(),
            ),
            Point2::new(
                center.u + radius * end_angle.cos(),
                center.v + radius * end_angle.sin(),
            ),
        ),
    }
}

fn point_in_circular_arc_loop(point: Point2, segments: &[ProfileBoundarySegment]) -> bool {
    segments
        .iter()
        .map(|segment| match segment {
            ProfileBoundarySegment::Line { start, end } => {
                let crosses_up = start.v <= point.v && point.v < end.v;
                let crosses_down = end.v <= point.v && point.v < start.v;
                if !(crosses_up || crosses_down)
                    || point.u
                        >= start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v)
                {
                    0
                } else if crosses_up {
                    1
                } else {
                    -1
                }
            }
            ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            } => horizontal_ray_arc_winding(point, *center, *radius, *start_angle, *end_angle),
        })
        .sum::<i32>()
        != 0
}

fn horizontal_ray_arc_winding(
    point: Point2,
    center: Point2,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> i32 {
    let ordinate = (point.v - center.v) / radius;
    if ordinate.abs() > 1.0 {
        return 0;
    }
    let principal = ordinate.asin();
    let sweep = end_angle - start_angle;
    let angles = if principal.cos().abs() <= 1.0e-12 {
        vec![principal]
    } else {
        vec![principal, std::f64::consts::PI - principal]
    };
    angles
        .into_iter()
        .filter(|angle| center.u + radius * angle.cos() > point.u)
        .filter_map(|angle| {
            let parameter = directed_angle_parameter(angle, start_angle, end_angle)?;
            let derivative = radius * angle.cos() * sweep;
            let second_derivative = -radius * angle.sin() * sweep * sweep;
            let direction = if parameter <= 1.0e-12 && derivative.abs() <= 1.0e-12 {
                second_derivative
            } else if parameter >= 1.0 - 1.0e-12 && derivative.abs() <= 1.0e-12 {
                -second_derivative
            } else {
                derivative
            };
            if parameter <= 1.0e-12 {
                (direction > 0.0).then_some(1)
            } else if parameter >= 1.0 - 1.0e-12 {
                (direction < 0.0).then_some(-1)
            } else if direction > 1.0e-12 {
                Some(1)
            } else if direction < -1.0e-12 {
                Some(-1)
            } else {
                None
            }
        })
        .sum()
}

fn directed_angle_parameter(angle: f64, start: f64, end: f64) -> Option<f64> {
    let sweep = end - start;
    if sweep == 0.0 || sweep.abs() > std::f64::consts::TAU {
        return None;
    }
    let displacement = if sweep > 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU)
    } else {
        -(start - angle).rem_euclid(std::f64::consts::TAU)
    };
    (displacement.abs() <= sweep.abs()).then_some(displacement / sweep)
}

fn point_boundary_segment_distance(point: Point2, segment: &ProfileBoundarySegment) -> f64 {
    match segment {
        ProfileBoundarySegment::Line { start, end } => {
            point_segment_distance(point, (*start, *end))
        }
        ProfileBoundarySegment::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let endpoint_distance = [*start_angle, *end_angle]
                .into_iter()
                .map(|angle| {
                    point_distance(
                        point,
                        Point2::new(
                            center.u + radius * angle.cos(),
                            center.v + radius * angle.sin(),
                        ),
                    )
                })
                .fold(f64::INFINITY, f64::min);
            let angle = (point.v - center.v).atan2(point.u - center.u);
            if directed_angle_parameter(angle, *start_angle, *end_angle).is_some() {
                endpoint_distance.min((point_distance(point, *center) - radius).abs())
            } else {
                endpoint_distance
            }
        }
    }
}

fn boundary_segment_max_distance(point: Point2, segment: &ProfileBoundarySegment) -> f64 {
    let (start, end) = boundary_segment_endpoints(segment);
    let endpoint_distance = point_distance(point, start).max(point_distance(point, end));
    let ProfileBoundarySegment::Arc {
        center,
        radius,
        start_angle,
        end_angle,
    } = segment
    else {
        return endpoint_distance;
    };
    let farthest_angle = (point.v - center.v).atan2(point.u - center.u) + std::f64::consts::PI;
    if directed_angle_parameter(farthest_angle, *start_angle, *end_angle).is_some() {
        endpoint_distance.max(point_distance(point, *center) + radius)
    } else {
        endpoint_distance
    }
}

pub(crate) fn point_in_polygon(point: Point2, vertices: &[Point2]) -> bool {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
        .filter(|(start, end)| {
            (start.v > point.v) != (end.v > point.v)
                && point.u < start.u + (point.v - start.v) * (end.u - start.u) / (end.v - start.v)
        })
        .count()
        % 2
        == 1
}

fn polygon_strictly_contains(outer: &[Point2], inner: &[Point2]) -> bool {
    inner.iter().all(|point| point_in_polygon(*point, outer))
        && !polygon_edges(outer).any(|outer_edge| {
            polygon_edges(inner).any(|inner_edge| segments_intersect(outer_edge, inner_edge))
        })
}

fn polygon_edges(vertices: &[Point2]) -> impl Iterator<Item = (Point2, Point2)> + '_ {
    vertices
        .iter()
        .copied()
        .zip(vertices.iter().copied().cycle().skip(1))
        .take(vertices.len())
}

pub(crate) fn point_segment_distance(point: Point2, (start, end): (Point2, Point2)) -> f64 {
    let du = end.u - start.u;
    let dv = end.v - start.v;
    let length_squared = du * du + dv * dv;
    if length_squared == 0.0 {
        return point_distance(point, start);
    }
    let parameter =
        (((point.u - start.u) * du + (point.v - start.v) * dv) / length_squared).clamp(0.0, 1.0);
    point_distance(
        point,
        Point2::new(start.u + parameter * du, start.v + parameter * dv),
    )
}

fn segment_distance(left: (Point2, Point2), right: (Point2, Point2)) -> f64 {
    if segments_intersect(left, right) {
        0.0
    } else {
        [
            point_segment_distance(left.0, right),
            point_segment_distance(left.1, right),
            point_segment_distance(right.0, left),
            point_segment_distance(right.1, left),
        ]
        .into_iter()
        .fold(f64::INFINITY, f64::min)
    }
}

fn segments_intersect(left: (Point2, Point2), right: (Point2, Point2)) -> bool {
    fn side(line: (Point2, Point2), point: Point2) -> f64 {
        (line.1.u - line.0.u) * (point.v - line.0.v) - (line.1.v - line.0.v) * (point.u - line.0.u)
    }

    let left_start = side(left, right.0);
    let left_end = side(left, right.1);
    let right_start = side(right, left.0);
    let right_end = side(right, left.1);
    if left_start == 0.0 && left_end == 0.0 && right_start == 0.0 && right_end == 0.0 {
        let overlaps = |a0: f64, a1: f64, b0: f64, b1: f64| {
            a0.min(a1) <= b0.max(b1) && b0.min(b1) <= a0.max(a1)
        };
        return overlaps(left.0.u, left.1.u, right.0.u, right.1.u)
            && overlaps(left.0.v, left.1.v, right.0.v, right.1.v);
    }
    left_start * left_end <= 0.0 && right_start * right_end <= 0.0
}

fn polygon_arc_loop_intersects(polygon: &[Point2], arc_loop: &[ProfileBoundarySegment]) -> bool {
    polygon_edges(polygon).any(|(start, end)| {
        let line = ProfileBoundarySegment::Line { start, end };
        arc_loop
            .iter()
            .any(|segment| boundary_segments_intersect(&line, segment))
    })
}

fn arc_loops_intersect(left: &[ProfileBoundarySegment], right: &[ProfileBoundarySegment]) -> bool {
    left.iter().any(|left| {
        right
            .iter()
            .any(|right| boundary_segments_intersect(left, right))
    })
}

fn boundary_segments_intersect(
    left: &ProfileBoundarySegment,
    right: &ProfileBoundarySegment,
) -> bool {
    match (left, right) {
        (
            ProfileBoundarySegment::Line {
                start: left_start,
                end: left_end,
            },
            ProfileBoundarySegment::Line {
                start: right_start,
                end: right_end,
            },
        ) => segments_intersect((*left_start, *left_end), (*right_start, *right_end)),
        (
            ProfileBoundarySegment::Line { start, end },
            ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            },
        )
        | (
            ProfileBoundarySegment::Arc {
                center,
                radius,
                start_angle,
                end_angle,
            },
            ProfileBoundarySegment::Line { start, end },
        ) => line_arc_intersects((*start, *end), *center, *radius, *start_angle, *end_angle),
        (
            ProfileBoundarySegment::Arc {
                center: left_center,
                radius: left_radius,
                start_angle: left_start,
                end_angle: left_end,
            },
            ProfileBoundarySegment::Arc {
                center: right_center,
                radius: right_radius,
                start_angle: right_start,
                end_angle: right_end,
            },
        ) => arcs_intersect(
            (*left_center, *left_radius, *left_start, *left_end),
            (*right_center, *right_radius, *right_start, *right_end),
        ),
    }
}

fn line_arc_intersects(
    (start, end): (Point2, Point2),
    center: Point2,
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> bool {
    let direction = Point2::new(end.u - start.u, end.v - start.v);
    let offset = Point2::new(start.u - center.u, start.v - center.v);
    let quadratic = direction.u * direction.u + direction.v * direction.v;
    if quadratic == 0.0 {
        return point_distance(start, center) == radius
            && directed_angle_parameter(
                (start.v - center.v).atan2(start.u - center.u),
                start_angle,
                end_angle,
            )
            .is_some();
    }
    let linear = 2.0 * (offset.u * direction.u + offset.v * direction.v);
    let constant = offset.u * offset.u + offset.v * offset.v - radius * radius;
    let discriminant = linear * linear - 4.0 * quadratic * constant;
    let error =
        64.0 * f64::EPSILON * (linear * linear + (4.0 * quadratic * constant).abs()).max(1.0);
    if discriminant < -error {
        return false;
    }
    let root = discriminant.max(0.0).sqrt();
    [-root, root].into_iter().any(|signed_root| {
        let parameter = (-linear + signed_root) / (2.0 * quadratic);
        if !(0.0..=1.0).contains(&parameter) {
            return false;
        }
        let point = Point2::new(
            start.u + parameter * direction.u,
            start.v + parameter * direction.v,
        );
        directed_angle_parameter(
            (point.v - center.v).atan2(point.u - center.u),
            start_angle,
            end_angle,
        )
        .is_some()
    })
}

fn arcs_intersect(left: (Point2, f64, f64, f64), right: (Point2, f64, f64, f64)) -> bool {
    let (left_center, left_radius, left_start, left_end) = left;
    let (right_center, right_radius, right_start, right_end) = right;
    let du = right_center.u - left_center.u;
    let dv = right_center.v - left_center.v;
    let distance_squared = du * du + dv * dv;
    if distance_squared == 0.0 && left_radius == right_radius {
        return [left_start, left_end]
            .into_iter()
            .any(|angle| directed_angle_parameter(angle, right_start, right_end).is_some())
            || [right_start, right_end]
                .into_iter()
                .any(|angle| directed_angle_parameter(angle, left_start, left_end).is_some());
    }
    if distance_squared == 0.0 {
        return false;
    }
    let distance = distance_squared.sqrt();
    if distance > left_radius + right_radius || distance < (left_radius - right_radius).abs() {
        return false;
    }
    let along = (left_radius * left_radius - right_radius * right_radius + distance_squared)
        / (2.0 * distance);
    let height_squared = left_radius * left_radius - along * along;
    let error = 64.0 * f64::EPSILON * (left_radius * left_radius + along * along).max(1.0);
    if height_squared < -error {
        return false;
    }
    let base = Point2::new(
        left_center.u + along * du / distance,
        left_center.v + along * dv / distance,
    );
    let height = height_squared.max(0.0).sqrt();
    [height, -height].into_iter().any(|signed_height| {
        let point = Point2::new(
            base.u - signed_height * dv / distance,
            base.v + signed_height * du / distance,
        );
        directed_angle_parameter(
            (point.v - left_center.v).atan2(point.u - left_center.u),
            left_start,
            left_end,
        )
        .is_some()
            && directed_angle_parameter(
                (point.v - right_center.v).atan2(point.u - right_center.u),
                right_start,
                right_end,
            )
            .is_some()
    })
}

pub(crate) fn historical_member_points_in_state(
    member: &DesignExtrudeSelectionMember,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    use crate::records::AsmHistoricalEntityKind;

    let kind =
        member
            .historical_entity_kind
            .or_else(|| match member.resolved_geometry.as_ref()? {
                SketchRelationOperand::Point { .. } => Some(AsmHistoricalEntityKind::Point),
                SketchRelationOperand::Curve { .. } => Some(AsmHistoricalEntityKind::Curve),
                SketchRelationOperand::Surface { .. } | SketchRelationOperand::Record { .. } => {
                    None
                }
            })?;
    let entity_ref = member
        .historical_entity_ref
        .or_else(|| i64::try_from(member.local_id).ok())?;
    historical_entity_positions(kind, entity_ref, topology)
}

pub(crate) fn historical_entity_positions(
    kind: crate::records::AsmHistoricalEntityKind,
    local_id: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<Point3>> {
    use crate::records::AsmHistoricalEntityKind;

    let mut positions = Vec::new();
    let edge_refs = match kind {
        AsmHistoricalEntityKind::Coedge => topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.coedge == local_id)
            .map(|coedge| coedge.edge)
            .collect::<Vec<_>>(),
        AsmHistoricalEntityKind::Edge => vec![local_id],
        AsmHistoricalEntityKind::Curve => topology
            .edge_curves
            .iter()
            .filter(|binding| binding.carrier == Some(local_id))
            .map(|binding| binding.entity)
            .collect(),
        AsmHistoricalEntityKind::Loop => topology
            .coedge_topology
            .iter()
            .filter(|coedge| coedge.owner_loop == local_id)
            .map(|coedge| coedge.edge)
            .collect(),
        AsmHistoricalEntityKind::Pcurve => topology
            .coedge_pcurves
            .iter()
            .filter(|binding| binding.carrier == Some(local_id))
            .filter_map(|binding| {
                topology
                    .coedge_topology
                    .iter()
                    .find(|coedge| coedge.coedge == binding.entity)
                    .map(|coedge| coedge.edge)
            })
            .collect(),
        AsmHistoricalEntityKind::Vertex => {
            positions.extend(historical_vertex_positions(topology, local_id));
            Vec::new()
        }
        AsmHistoricalEntityKind::Point => {
            positions.extend(
                topology
                    .point_positions
                    .iter()
                    .filter(|point| point.point == local_id)
                    .map(|point| point.position),
            );
            Vec::new()
        }
        AsmHistoricalEntityKind::Face => {
            positions.extend(historical_face_points(local_id, topology)?);
            Vec::new()
        }
        AsmHistoricalEntityKind::Surface => {
            let faces = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.carrier == local_id)
                .map(|binding| binding.entity)
                .collect::<Vec<_>>();
            if faces.is_empty() {
                return None;
            }
            for face in faces {
                positions.extend(historical_face_points(face, topology)?);
            }
            Vec::new()
        }
        AsmHistoricalEntityKind::Body
        | AsmHistoricalEntityKind::Region
        | AsmHistoricalEntityKind::Shell => {
            let faces = historical_owned_faces(kind, local_id, topology)?;
            for face in faces {
                positions.extend(historical_face_points(face, topology)?);
            }
            Vec::new()
        }
    };
    for edge_ref in edge_refs {
        let edge = topology
            .edge_vertices
            .iter()
            .find(|edge| edge.edge == edge_ref)?;
        let start = historical_vertex_positions(topology, edge.start_vertex).collect::<Vec<_>>();
        let end = historical_vertex_positions(topology, edge.end_vertex).collect::<Vec<_>>();
        if start.is_empty() || end.is_empty() {
            return None;
        }
        positions.extend(start);
        positions.extend(end);
    }
    (!positions.is_empty()).then_some(positions)
}

pub(crate) fn historical_owned_faces(
    kind: crate::records::AsmHistoricalEntityKind,
    local_id: i64,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<Vec<i64>> {
    use crate::records::AsmHistoricalEntityKind;

    let relation_members = |relations: &[crate::history_records::AsmHistoricalRelation], owner| {
        let mut matches = relations
            .iter()
            .filter(|relation| relation.owner_ref == owner);
        let members = matches.next()?.member_refs.clone();
        matches.next().is_none().then_some(members)
    };
    let regions = match kind {
        AsmHistoricalEntityKind::Body => relation_members(&topology.body_regions, local_id)?,
        AsmHistoricalEntityKind::Region => vec![local_id],
        AsmHistoricalEntityKind::Shell => Vec::new(),
        _ => return None,
    };
    let shells = if kind == AsmHistoricalEntityKind::Shell {
        vec![local_id]
    } else {
        let mut shells = Vec::new();
        for region in regions {
            shells.extend(relation_members(&topology.region_shells, region)?);
        }
        shells
    };
    let mut faces = Vec::new();
    for shell in shells {
        faces.extend(relation_members(&topology.shell_faces, shell)?);
    }
    faces.sort_unstable();
    faces.dedup();
    (!faces.is_empty()).then_some(faces)
}

fn historical_vertex_positions(
    topology: &crate::history_records::AsmHistoricalTopology,
    vertex_ref: i64,
) -> impl Iterator<Item = Point3> + '_ {
    topology
        .vertex_points
        .iter()
        .filter(move |binding| binding.entity == vertex_ref)
        .filter_map(|binding| {
            topology
                .point_positions
                .iter()
                .find(|point| point.point == binding.carrier)
                .map(|point| point.position)
        })
}

pub(crate) fn project_to_sketch(
    sketch: &cadmpeg_ir::sketches::Sketch,
    point: Point3,
) -> Option<Point2> {
    let (origin, normal, u_axis) = sketch.resolved_placement()?;
    let x = point.x - origin.x;
    let y = point.y - origin.y;
    let z = point.z - origin.z;
    let v_axis = Vector3::new(
        normal.y * u_axis.z - normal.z * u_axis.y,
        normal.z * u_axis.x - normal.x * u_axis.z,
        normal.x * u_axis.y - normal.y * u_axis.x,
    );
    Some(Point2::new(
        x * u_axis.x + y * u_axis.y + z * u_axis.z,
        x * v_axis.x + y * v_axis.y + z * v_axis.z,
    ))
}

pub(crate) fn point_on_sketch_entity(
    point: Point2,
    entity: &cadmpeg_ir::sketches::SketchEntity,
    tolerance: f64,
) -> bool {
    use cadmpeg_ir::sketches::SketchGeometry;

    match &entity.geometry {
        SketchGeometry::Line { start, end } => {
            let dx = end.u - start.u;
            let dy = end.v - start.v;
            let length_squared = dx * dx + dy * dy;
            if length_squared == 0.0 {
                return point_distance(point, *start) <= tolerance;
            }
            let t = ((point.u - start.u) * dx + (point.v - start.v) * dy) / length_squared;
            if !(-tolerance..=1.0 + tolerance).contains(&t) {
                return false;
            }
            point_distance(point, Point2::new(start.u + t * dx, start.v + t * dy)) <= tolerance
        }
        SketchGeometry::Circle { center, radius } => {
            (point_distance(point, *center) - radius.0).abs() <= tolerance
        }
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => {
            let radial_error = (point_distance(point, *center) - radius.0).abs();
            if radial_error > tolerance {
                return false;
            }
            let angle = (point.v - center.v).atan2(point.u - center.u);
            angle_in_sweep(angle, start_angle.0, end_angle.0, tolerance / radius.0)
        }
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle,
            end_angle,
        } if major_radius.0 > 0.0 && minor_radius.0 > 0.0 => {
            let du = point.u - center.u;
            let dv = point.v - center.v;
            let cosine = major_angle.0.cos();
            let sine = major_angle.0.sin();
            let local_u = du * cosine + dv * sine;
            let local_v = -du * sine + dv * cosine;
            let parameter = (local_v / minor_radius.0).atan2(local_u / major_radius.0);
            let boundary = Point2::new(
                center.u + major_radius.0 * parameter.cos() * cosine
                    - minor_radius.0 * parameter.sin() * sine,
                center.v
                    + major_radius.0 * parameter.cos() * sine
                    + minor_radius.0 * parameter.sin() * cosine,
            );
            if point_distance(point, boundary) > tolerance {
                return false;
            }
            match (start_angle, end_angle) {
                (None, None) => true,
                (Some(start), Some(end)) => angle_in_sweep(
                    parameter,
                    start.0,
                    end.0,
                    tolerance / major_radius.0.min(minor_radius.0),
                ),
                _ => false,
            }
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => cadmpeg_ir::eval::nurbs_pcurve_contains_point(
            *degree,
            knots,
            control_points,
            weights.as_deref(),
            point,
            tolerance,
        )
        .unwrap_or(false),
        _ => false,
    }
}

pub(crate) fn angle_in_sweep(angle: f64, start: f64, end: f64, tolerance: f64) -> bool {
    let sweep = end - start;
    if sweep.abs() >= std::f64::consts::TAU - tolerance {
        return true;
    }
    if sweep >= 0.0 {
        (angle - start).rem_euclid(std::f64::consts::TAU) <= sweep + tolerance
    } else {
        (start - angle).rem_euclid(std::f64::consts::TAU) <= -sweep + tolerance
    }
}

fn point_distance(a: Point2, b: Point2) -> f64 {
    ((a.u - b.u).powi(2) + (a.v - b.v).powi(2)).sqrt()
}

pub(crate) fn closed_sketch_profiles(
    sketch: &cadmpeg_ir::sketches::SketchId,
    entities: &[cadmpeg_ir::sketches::SketchEntity],
    linear_tolerance: f64,
) -> Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>> {
    use cadmpeg_ir::sketches::{SketchEntityUse, SketchGeometry};

    if !linear_tolerance.is_finite() || linear_tolerance <= 0.0 {
        return Vec::new();
    }
    let mut profiles = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && !entity.construction)
        .filter(|entity| {
            matches!(
                entity.geometry,
                SketchGeometry::Circle { .. }
                    | SketchGeometry::Ellipse {
                        start_angle: None,
                        end_angle: None,
                        ..
                    }
            )
        })
        .map(|entity| {
            vec![SketchEntityUse {
                entity: entity.id.clone(),
                reversed: false,
            }]
        })
        .collect::<Vec<_>>();
    let edges = entities
        .iter()
        .filter(|entity| &entity.sketch == sketch && !entity.construction)
        .filter_map(|entity| sketch_entity_endpoints(entity).map(|ends| (entity, ends)))
        .collect::<Vec<_>>();
    if edges.is_empty() {
        profiles.sort_by_key(|profile| profile[0].entity.clone());
        return profiles;
    }

    let endpoints = edges
        .iter()
        .flat_map(|(_, [start, end])| [*start, *end])
        .collect::<Vec<_>>();
    let mut parents = (0..endpoints.len()).collect::<Vec<_>>();
    let mut endpoint_cells = HashMap::<(i64, i64), Vec<usize>>::new();
    for (endpoint, point) in endpoints.iter().copied().enumerate() {
        let cell = (
            (point.u / linear_tolerance).floor() as i64,
            (point.v / linear_tolerance).floor() as i64,
        );
        for u_offset in -1..=1 {
            for v_offset in -1..=1 {
                let adjacent = (
                    cell.0.saturating_add(u_offset),
                    cell.1.saturating_add(v_offset),
                );
                for candidate in endpoint_cells.get(&adjacent).into_iter().flatten() {
                    if sketch_endpoints_close(point, endpoints[*candidate], linear_tolerance) {
                        union_endpoint_nodes(&mut parents, endpoint, *candidate);
                    }
                }
            }
        }
        endpoint_cells.entry(cell).or_default().push(endpoint);
    }
    let edge_nodes = (0..edges.len())
        .map(|edge| {
            [
                endpoint_root(&mut parents, edge * 2),
                endpoint_root(&mut parents, edge * 2 + 1),
            ]
        })
        .collect::<Vec<_>>();
    let mut adjacency = HashMap::<usize, Vec<usize>>::new();
    for (edge, [start, end]) in edge_nodes.iter().copied().enumerate() {
        adjacency.entry(start).or_default().push(edge);
        adjacency.entry(end).or_default().push(edge);
    }
    for incident in adjacency.values_mut() {
        incident.sort_by_key(|edge| edges[*edge].0.id.clone());
    }

    let mut visited = vec![false; edges.len()];
    let mut order = (0..edges.len()).collect::<Vec<_>>();
    order.sort_by_key(|edge| edges[*edge].0.id.clone());
    for first_edge in order {
        if visited[first_edge] {
            continue;
        }
        let mut component = Vec::new();
        let mut pending = vec![first_edge];
        let mut component_seen = HashSet::new();
        while let Some(edge) = pending.pop() {
            if !component_seen.insert(edge) {
                continue;
            }
            component.push(edge);
            for node in edge_nodes[edge] {
                pending.extend(adjacency[&node].iter().copied());
            }
        }
        let component_nodes = component
            .iter()
            .flat_map(|edge| edge_nodes[*edge])
            .collect::<HashSet<_>>();
        if component_nodes
            .iter()
            .any(|node| adjacency[node].len() != 2)
        {
            if component
                .iter()
                .all(|edge| matches!(edges[*edge].0.geometry, SketchGeometry::Line { .. }))
            {
                profiles.extend(branched_line_profiles(
                    &component,
                    &edges,
                    &edge_nodes,
                    &adjacency,
                    linear_tolerance,
                ));
            }
            for edge in component {
                visited[edge] = true;
            }
            continue;
        }

        component.sort_by_key(|edge| edges[*edge].0.id.clone());
        let first_edge = component[0];
        let start_node = edge_nodes[first_edge][0];
        let mut current_node = edge_nodes[first_edge][1];
        let mut profile = vec![SketchEntityUse {
            entity: edges[first_edge].0.id.clone(),
            reversed: false,
        }];
        visited[first_edge] = true;
        while current_node != start_node {
            let Some(next_edge) = adjacency[&current_node]
                .iter()
                .copied()
                .find(|edge| !visited[*edge])
            else {
                profile.clear();
                break;
            };
            let [stored_start, stored_end] = edge_nodes[next_edge];
            let reversed = stored_end == current_node;
            current_node = if reversed { stored_start } else { stored_end };
            visited[next_edge] = true;
            profile.push(SketchEntityUse {
                entity: edges[next_edge].0.id.clone(),
                reversed,
            });
        }
        if !profile.is_empty() && component.iter().all(|edge| visited[*edge]) {
            profiles.push(profile);
        }
    }
    profiles.sort_by_key(|profile| profile[0].entity.clone());
    profiles
}

fn branched_line_profiles(
    component: &[usize],
    edges: &[(&cadmpeg_ir::sketches::SketchEntity, [Point2; 2])],
    edge_nodes: &[[usize; 2]],
    adjacency: &HashMap<usize, Vec<usize>>,
    linear_tolerance: f64,
) -> Vec<Vec<cadmpeg_ir::sketches::SketchEntityUse>> {
    use cadmpeg_ir::sketches::SketchEntityUse;

    let component = component.iter().copied().collect::<HashSet<_>>();
    let mut outgoing = HashMap::<usize, Vec<usize>>::new();
    for edge in &component {
        outgoing
            .entry(edge_nodes[*edge][0])
            .or_default()
            .push(edge * 2);
        outgoing
            .entry(edge_nodes[*edge][1])
            .or_default()
            .push(edge * 2 + 1);
    }
    for half_edges in outgoing.values_mut() {
        half_edges.sort_by(|first, second| {
            let angle = |half_edge: usize| {
                let edge = half_edge / 2;
                let [start, end] = edges[edge].1;
                let (from, to) = if half_edge.is_multiple_of(2) {
                    (start, end)
                } else {
                    (end, start)
                };
                (to.v - from.v).atan2(to.u - from.u)
            };
            angle(*first)
                .total_cmp(&angle(*second))
                .then_with(|| edges[*first / 2].0.id.cmp(&edges[*second / 2].0.id))
                .then_with(|| first.cmp(second))
        });
    }

    let mut next = HashMap::new();
    for edge in &component {
        for half_edge in [edge * 2, edge * 2 + 1] {
            let destination = edge_nodes[*edge][usize::from(half_edge.is_multiple_of(2))];
            let around = &outgoing[&destination];
            let twin = half_edge ^ 1;
            let twin_position = around
                .iter()
                .position(|candidate| *candidate == twin)
                .expect("each line half-edge has a twin at its destination");
            next.insert(
                half_edge,
                around[(twin_position + around.len() - 1) % around.len()],
            );
        }
    }

    let mut profiles = Vec::new();
    let mut visited = HashSet::new();
    let mut starts = component
        .iter()
        .flat_map(|edge| [edge * 2, edge * 2 + 1])
        .collect::<Vec<_>>();
    starts.sort_by_key(|half_edge| (edges[*half_edge / 2].0.id.clone(), half_edge % 2));
    for start in starts {
        if visited.contains(&start) {
            continue;
        }
        let mut current = start;
        let mut profile = Vec::new();
        let mut twice_area = 0.0;
        loop {
            if !visited.insert(current) {
                if current != start {
                    profile.clear();
                }
                break;
            }
            let edge = current / 2;
            let [stored_start, stored_end] = edges[edge].1;
            let reversed = !current.is_multiple_of(2);
            let (from, to) = if reversed {
                (stored_end, stored_start)
            } else {
                (stored_start, stored_end)
            };
            twice_area += from.u * to.v - from.v * to.u;
            profile.push(SketchEntityUse {
                entity: edges[edge].0.id.clone(),
                reversed,
            });
            current = next[&current];
        }
        if !profile.is_empty() && twice_area > 2.0 * linear_tolerance * linear_tolerance {
            profiles.push(profile);
        }
    }

    debug_assert!(component.iter().all(|edge| {
        edge_nodes[*edge]
            .iter()
            .all(|node| adjacency[node].contains(edge))
    }));
    profiles
}

pub(crate) fn sketch_entity_endpoints(
    entity: &cadmpeg_ir::sketches::SketchEntity,
) -> Option<[Point2; 2]> {
    use cadmpeg_ir::sketches::SketchGeometry;

    match &entity.geometry {
        SketchGeometry::Line { start, end } => Some([*start, *end]),
        SketchGeometry::Arc {
            center,
            radius,
            start_angle,
            end_angle,
        } => Some([
            Point2::new(
                center.u + radius.0 * start_angle.0.cos(),
                center.v + radius.0 * start_angle.0.sin(),
            ),
            Point2::new(
                center.u + radius.0 * end_angle.0.cos(),
                center.v + radius.0 * end_angle.0.sin(),
            ),
        ]),
        SketchGeometry::Ellipse {
            center,
            major_angle,
            major_radius,
            minor_radius,
            start_angle: Some(start_angle),
            end_angle: Some(end_angle),
        } => {
            let point_at = |parameter: f64| {
                let x = major_radius.0 * parameter.cos();
                let y = minor_radius.0 * parameter.sin();
                Point2::new(
                    center.u + x * major_angle.0.cos() - y * major_angle.0.sin(),
                    center.v + x * major_angle.0.sin() + y * major_angle.0.cos(),
                )
            };
            Some([point_at(start_angle.0), point_at(end_angle.0)])
        }
        SketchGeometry::Nurbs {
            degree,
            knots,
            control_points,
            weights,
            periodic: false,
        } => {
            let degree_index = usize::try_from(*degree).ok()?;
            let start_parameter = *knots.get(degree_index)?;
            let end_parameter = *knots.get(control_points.len())?;
            Some([
                cadmpeg_ir::eval::nurbs_pcurve_uv(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    start_parameter,
                )?,
                cadmpeg_ir::eval::nurbs_pcurve_uv(
                    *degree,
                    knots,
                    control_points,
                    weights.as_deref(),
                    end_parameter,
                )?,
            ])
        }
        _ => None,
    }
}

fn sketch_endpoints_close(first: Point2, second: Point2, tolerance: f64) -> bool {
    (first.u - second.u).hypot(first.v - second.v) <= tolerance
}

fn endpoint_root(parents: &mut [usize], node: usize) -> usize {
    if parents[node] != node {
        parents[node] = endpoint_root(parents, parents[node]);
    }
    parents[node]
}

fn union_endpoint_nodes(parents: &mut [usize], first: usize, second: usize) {
    let first = endpoint_root(parents, first);
    let second = endpoint_root(parents, second);
    if first != second {
        parents[second] = first;
    }
}
