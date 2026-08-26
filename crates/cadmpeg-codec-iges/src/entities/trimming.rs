// SPDX-License-Identifier: Apache-2.0
//! Face-local trimmed-surface projection.

use super::composite::{bounded_nurbs_for_curve_with_tolerance, CompositeIndex};
use super::evaluation;
use super::geometry::{
    entity_loss, linear_nurbs_parameters, planar_polyline_has_self_intersection,
    planar_polylines_intersect, plane_coordinates, source_object, BoundaryEndpoint,
    BoundaryVertexDerivation, BoundaryVertexSourceEndpoint, ProjectionOutcome,
};
use crate::directory::DirectoryEntry;
use crate::global::ProjectedGlobal;
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, TokenValue};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, Pcurve, PcurveGeometry, ProceduralSurface,
    ProceduralSurfaceDefinition, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralSurfaceId,
    RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::report::LossNote;
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::CadIr;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
struct BoundarySegment {
    model_curve: u32,
    pcurves: Vec<u32>,
    sense: Sense,
    parameter_curves_authoritative: bool,
}

#[derive(Clone)]
struct BoundaryDefinition {
    surface: u32,
    segments: Vec<BoundarySegment>,
}

struct BoundaryItem {
    segment: BoundarySegment,
    model_curve: CurveId,
    source_edge: Edge,
    start: Point3,
    end: Point3,
    pcurves: Vec<(PcurveGeometry, [f64; 2])>,
}

#[derive(Debug)]
enum BoundaryEdgeSelectionError {
    MissingEndpoints,
    InvalidRange,
    Ambiguous,
    PcurveDisagreement,
}

fn boundary_parameter_loss(entry: &DirectoryEntry, message: impl Into<String>) -> LossNote {
    IgesLossCode::BoundaryPcurveOutsideSupportDomain
        .note(format!(
            "IGES entity type {} form {}: {}",
            entry.entity_type,
            entry.form,
            message.into()
        ))
        .with_provenance(entry.loss_provenance())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryVertexClusterError {
    InvalidTolerance,
    NonTransitive,
}

#[derive(Debug, PartialEq)]
struct BoundaryVertexCluster {
    representative: Point3,
    members: Vec<usize>,
}

fn pointer(record: &ParameterRecord, index: usize) -> Option<u32> {
    record.integer(index).and_then(|value| {
        let sequence = u32::try_from(value).ok()?;
        (sequence % 2 == 1).then_some(sequence)
    })
}

fn close(left: Point3, right: Point3, tolerance: f64) -> bool {
    tolerance.is_finite() && tolerance >= 0.0 && left.distance(right) <= tolerance
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FaceTolerancePolicy {
    topology_sewing: f64,
}

impl FaceTolerancePolicy {
    fn from_global(global: &ProjectedGlobal, points: impl Iterator<Item = Point3>) -> Self {
        let carrier_agreement = global.minimum_resolution_mm();
        let coordinate_quantum = coordinate_quantum(global, points);
        Self {
            topology_sewing: carrier_agreement.max(coordinate_quantum),
        }
    }
}

fn coordinate_quantum(global: &ProjectedGlobal, points: impl Iterator<Item = Point3>) -> f64 {
    let magnitude = points.fold(0.0_f64, |magnitude, point| {
        magnitude
            .max(point.x.abs())
            .max(point.y.abs())
            .max(point.z.abs())
    });
    if magnitude > 0.0 {
        10.0_f64.powf(
            magnitude.log10().floor() - f64::from(global.single_precision_significance()) + 1.0,
        )
    } else {
        0.0
    }
}

fn point_order(left: Point3, right: Point3) -> Ordering {
    left.x
        .total_cmp(&right.x)
        .then_with(|| left.y.total_cmp(&right.y))
        .then_with(|| left.z.total_cmp(&right.z))
}

fn find_cluster_root(parents: &mut [usize], index: usize) -> usize {
    if parents[index] == index {
        return index;
    }
    let root = find_cluster_root(parents, parents[index]);
    parents[index] = root;
    root
}

fn cluster_boundary_positions(
    positions: &[Point3],
    tolerance: f64,
) -> Result<Vec<BoundaryVertexCluster>, BoundaryVertexClusterError> {
    if !tolerance.is_finite() || tolerance < 0.0 {
        return Err(BoundaryVertexClusterError::InvalidTolerance);
    }
    let mut parents = (0..positions.len()).collect::<Vec<_>>();
    for (left_index, left) in positions.iter().enumerate() {
        for (right_index, right) in positions.iter().enumerate().skip(left_index + 1) {
            if !close(*left, *right, tolerance) {
                continue;
            }
            let left_root = find_cluster_root(&mut parents, left_index);
            let right_root = find_cluster_root(&mut parents, right_index);
            if left_root != right_root {
                parents[right_root] = left_root;
            }
        }
    }
    let mut members_by_root = BTreeMap::<usize, Vec<usize>>::new();
    for index in 0..positions.len() {
        let root = find_cluster_root(&mut parents, index);
        members_by_root.entry(root).or_default().push(index);
    }
    let mut clusters = Vec::with_capacity(members_by_root.len());
    for members in members_by_root.into_values() {
        if members.iter().enumerate().any(|(offset, left)| {
            members
                .iter()
                .skip(offset + 1)
                .any(|right| !close(positions[*left], positions[*right], tolerance))
        }) {
            return Err(BoundaryVertexClusterError::NonTransitive);
        }
        let representative = members
            .iter()
            .copied()
            .min_by(|left, right| {
                point_order(positions[*left], positions[*right]).then_with(|| left.cmp(right))
            })
            .map(|index| positions[index])
            .ok_or(BoundaryVertexClusterError::NonTransitive)?;
        clusters.push(BoundaryVertexCluster {
            representative,
            members,
        });
    }
    clusters.sort_by_key(|cluster| cluster.members[0]);
    Ok(clusters)
}

fn create_boundary_vertices(
    candidate: &mut ModelDraft,
    stem: &str,
    source_entity: &str,
    boundary: usize,
    source_endpoints: &[BoundaryVertexSourceEndpoint],
    tolerance: f64,
) -> Result<(Vec<VertexId>, Vec<BoundaryVertexDerivation>), BoundaryVertexClusterError> {
    let positions = source_endpoints
        .iter()
        .map(|endpoint| endpoint.position)
        .collect::<Vec<_>>();
    let clusters = cluster_boundary_positions(&positions, tolerance)?;
    let mut vertex_ids = (0..positions.len())
        .map(|_| None)
        .collect::<Vec<Option<VertexId>>>();
    let mut derivations = Vec::new();
    for (index, cluster) in clusters.into_iter().enumerate() {
        let point_id = PointId(format!("iges:model:point#{stem}:{boundary}:{index}"));
        let vertex_id = VertexId(format!("iges:model:vertex#{stem}:{boundary}:{index}"));
        candidate.model_mut().points.push(Point {
            source_object: None,
            id: point_id.clone(),
            position: cluster.representative,
        });
        candidate.model_mut().vertices.push(Vertex {
            id: vertex_id.clone(),
            point: point_id,
            tolerance: Some(tolerance),
        });
        let source_endpoints = cluster
            .members
            .iter()
            .map(|member| source_endpoints[*member].clone())
            .collect();
        derivations.push(BoundaryVertexDerivation {
            source_entity: source_entity.into(),
            vertex: vertex_id.clone(),
            representative: cluster.representative,
            tolerance,
            source_endpoints,
        });
        for member in cluster.members {
            vertex_ids[member] = Some(vertex_id.clone());
        }
    }
    Ok((vertex_ids.into_iter().flatten().collect(), derivations))
}

fn point_position(index: &ModelIndex<'_>, id: &VertexId) -> Option<Point3> {
    let point_id = &index.vertices(&id.0)?.point;
    index.points(&point_id.0).map(|point| point.position)
}

pub(super) struct PcurveSupport<'a> {
    pub(super) surface_id: &'a SurfaceId,
    pub(super) geometry: &'a SurfaceGeometry,
    pub(super) factor: f64,
}

pub(super) fn pcurve_geometry(
    ir: &CadIr,
    sequence: u32,
    support: &PcurveSupport<'_>,
    tolerance: Option<f64>,
    ctx: Option<&DecodeContext<'_>>,
    composite_index: Option<&CompositeIndex>,
) -> Option<(PcurveGeometry, [f64; 2])> {
    let curve_id = CurveId(format!("iges:model:curve#D{sequence}"));
    let (nurbs, range) =
        bounded_nurbs_for_curve_with_tolerance(ir, &curve_id, tolerance, ctx, composite_index)?;
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.surface == *support.surface_id);
    let parameter_map = match procedural {
        Some(procedural)
            if matches!(
                &procedural.definition,
                ProceduralSurfaceDefinition::Extrusion { .. }
                    | ProceduralSurfaceDefinition::Revolution { .. }
            ) =>
        {
            Some(procedural_pcurve_parameter_map(ir, &procedural.id))
        }
        _ => None,
    };
    let (u_factor, u_offset, v_factor, v_offset) = match parameter_map {
        Some(Some(parameter_map)) => parameter_map,
        Some(None) => return None,
        None => match support.geometry {
            SurfaceGeometry::Plane { .. } => (1.0, 0.0, 1.0, 0.0),
            SurfaceGeometry::Cylinder { .. } | SurfaceGeometry::Cone { .. } => {
                (1.0 / support.factor, 0.0, 1.0, 0.0)
            }
            SurfaceGeometry::Sphere { .. }
            | SurfaceGeometry::Torus { .. }
            | SurfaceGeometry::Nurbs(_) => (1.0 / support.factor, 0.0, 1.0 / support.factor, 0.0),
            SurfaceGeometry::Procedural { construction } => {
                procedural_pcurve_parameter_map(ir, construction)?
            }
            SurfaceGeometry::Polygonal { .. }
            | SurfaceGeometry::Transformed { .. }
            | SurfaceGeometry::Unknown { .. } => return None,
        },
    };
    Some((
        PcurveGeometry::Nurbs {
            degree: nurbs.degree,
            knots: nurbs.knots,
            control_points: nurbs
                .control_points
                .iter()
                .map(|point| {
                    Point2::new(
                        point.x.mul_add(u_factor, u_offset),
                        point.y.mul_add(v_factor, v_offset),
                    )
                })
                .collect(),
            weights: nurbs.weights,
            periodic: nurbs.periodic,
        },
        range,
    ))
}

fn line_directrix(ir: &CadIr, curve_id: &CurveId) -> bool {
    fn is_line(geometry: &CurveGeometry, depth: usize) -> bool {
        if depth > 256 {
            return false;
        }
        match geometry {
            CurveGeometry::Line { .. } => true,
            CurveGeometry::Transformed { basis, .. } => is_line(basis, depth + 1),
            _ => false,
        }
    }

    ir.model
        .curves
        .iter()
        .find(|curve| curve.id == *curve_id)
        .is_some_and(|curve| is_line(&curve.geometry, 0))
}

fn affine_parameter_map(source: [f64; 2], target: [f64; 2]) -> Option<(f64, f64)> {
    let source_width = source[1] - source[0];
    let target_width = target[1] - target[0];
    if !source
        .iter()
        .chain(target.iter())
        .all(|value| value.is_finite())
        || source_width <= 0.0
        || target_width <= 0.0
    {
        return None;
    }
    let scale = target_width / source_width;
    let offset = target[0] - source[0] * scale;
    (scale.is_finite() && offset.is_finite()).then_some((scale, offset))
}

fn procedural_pcurve_parameter_map(
    ir: &CadIr,
    construction: &ProceduralSurfaceId,
) -> Option<(f64, f64, f64, f64)> {
    let procedural = ir
        .model
        .procedural_surfaces
        .iter()
        .find(|procedural| procedural.id == *construction)?;
    let Some([Some(carrier_start), Some(carrier_end), _, _]) = procedural.record_bounds else {
        return None;
    };
    let carrier_interval = [carrier_start, carrier_end];
    if !carrier_interval.iter().all(|value| value.is_finite())
        || carrier_interval[0] >= carrier_interval[1]
    {
        return None;
    }
    let mut u_map = (1.0, 0.0);
    let mut v_map = (1.0, 0.0);
    match &procedural.definition {
        ProceduralSurfaceDefinition::Extrusion {
            directrix,
            parameter_interval,
            ..
        } => {
            if line_directrix(ir, directrix) {
                u_map = affine_parameter_map([0.0, 1.0], carrier_interval)?;
            } else if let Some(parameter_interval) = parameter_interval {
                u_map = affine_parameter_map(*parameter_interval, carrier_interval)?;
            }
        }
        ProceduralSurfaceDefinition::Revolution {
            directrix,
            angular_interval,
            angular_parameter_interval,
            parameter_interval,
            transposed,
            ..
        } => {
            let directrix_map = if line_directrix(ir, directrix) {
                affine_parameter_map([0.0, 1.0], carrier_interval)?
            } else if let Some(parameter_interval) = parameter_interval {
                affine_parameter_map(*parameter_interval, carrier_interval)?
            } else {
                (1.0, 0.0)
            };
            let angular_map = match angular_parameter_interval {
                Some(parameter_interval) => {
                    affine_parameter_map(*parameter_interval, *angular_interval)?
                }
                None => (1.0, 0.0),
            };
            if *transposed {
                u_map = angular_map;
                v_map = directrix_map;
            } else {
                u_map = directrix_map;
                v_map = angular_map;
            }
        }
        _ => return None,
    }
    Some((u_map.0, u_map.1, v_map.0, v_map.1))
}

fn linear_model_nurbs_points(nurbs: &NurbsCurve, range: [f64; 2]) -> Option<Vec<Point3>> {
    if nurbs.weights.as_ref().is_some_and(|weights| {
        weights.len() != nurbs.control_points.len() || weights.iter().any(|weight| *weight != 1.0)
    }) {
        return None;
    }
    linear_nurbs_parameters(
        nurbs.degree,
        &nurbs.knots,
        nurbs.control_points.len(),
        nurbs.periodic,
        range,
    )?
    .into_iter()
    .map(|parameter| {
        cadmpeg_ir::eval::nurbs_curve_point(
            nurbs.degree,
            &nurbs.knots,
            &nurbs.control_points,
            None,
            parameter,
        )
        .filter(|point| point.x.is_finite() && point.y.is_finite() && point.z.is_finite())
    })
    .collect()
}

fn linear_pcurve_points(geometry: &PcurveGeometry, range: [f64; 2]) -> Option<Vec<[f64; 2]>> {
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        periodic,
    } = geometry
    else {
        return None;
    };
    if weights.as_ref().is_some_and(|weights| {
        weights.len() != control_points.len() || weights.iter().any(|weight| *weight != 1.0)
    }) {
        return None;
    }
    linear_nurbs_parameters(*degree, knots, control_points.len(), *periodic, range)?
        .into_iter()
        .map(|parameter| {
            evaluation::pcurve(geometry, parameter)
                .map(|point| [point.u, point.v])
                .filter(|point| point.iter().all(|coordinate| coordinate.is_finite()))
        })
        .collect()
}

fn append_path<T: Copy + PartialEq>(target: &mut Vec<T>, path: Vec<T>) -> Option<()> {
    let first = path.first().copied()?;
    if target.last().is_some_and(|last| *last != first) {
        return None;
    }
    if target.is_empty() {
        target.extend(path);
    } else {
        target.extend(path.into_iter().skip(1));
    }
    Some(())
}

fn normalize_model_ring_endpoints(points: &mut [Point3], tolerance: f64) {
    if let (Some(first), Some(last)) = (points.first().copied(), points.last_mut()) {
        if close(first, *last, tolerance) {
            *last = first;
        }
    }
}

fn normalize_parameter_ring_endpoints(points: &mut [[f64; 2]], tolerance: f64) {
    if let (Some(first), Some(last)) = (points.first().copied(), points.last_mut()) {
        let distance = (first[0] - last[0]).hypot(first[1] - last[1]);
        if distance.is_finite() && distance <= tolerance {
            *last = first;
        }
    }
}

fn linear_boundary_model_points(
    items: &[BoundaryItem],
    index: &ModelIndex<'_>,
    closure_tolerance: f64,
) -> Option<Vec<Point3>> {
    let mut points = Vec::new();
    for item in items {
        let curve = index.curves(&item.model_curve.0)?;
        let mut curve_points = match &curve.geometry {
            CurveGeometry::Line { .. } => vec![item.start, item.end],
            CurveGeometry::Nurbs(nurbs) => {
                linear_model_nurbs_points(nurbs, item.source_edge.param_range?)?
            }
            _ => return None,
        };
        if curve_points.first().copied() != Some(item.start)
            || curve_points.last().copied() != Some(item.end)
        {
            return None;
        }
        if item.segment.sense == Sense::Reversed {
            curve_points.reverse();
        }
        append_path(&mut points, curve_points)?;
    }
    normalize_model_ring_endpoints(&mut points, closure_tolerance);
    Some(points)
}

#[derive(Clone)]
enum LinearBoundaryGeometry {
    Parameter(Vec<[f64; 2]>),
    Model(Vec<[f64; 2]>),
}

fn linear_boundary_geometry(
    items: &[BoundaryItem],
    index: &ModelIndex<'_>,
    support: &SurfaceGeometry,
    resolution: f64,
    closure_tolerance: f64,
    use_parameter_curves: bool,
) -> Option<LinearBoundaryGeometry> {
    let SurfaceGeometry::Plane { origin, normal, .. } = support else {
        return None;
    };
    let model_points = linear_boundary_model_points(items, index, closure_tolerance)?;
    let model_plane = (*origin, *normal);
    if items.iter().any(|item| {
        let Some(curve) = index.curves(&item.model_curve.0) else {
            return true;
        };
        !super::geometry::curve_geometry_coplanar(
            &curve.geometry,
            index,
            cadmpeg_ir::transform::Transform::identity(),
            model_plane,
            resolution,
            &mut BTreeSet::new(),
        )
    }) {
        return None;
    }
    let model_coordinates = plane_coordinates(&model_points, model_plane)?;
    if use_parameter_curves
        && items
            .iter()
            .any(|item| item.segment.parameter_curves_authoritative)
    {
        if !items
            .iter()
            .all(|item| item.segment.parameter_curves_authoritative)
        {
            return None;
        }
        let mut parameter_points = Vec::new();
        for item in items {
            if item.pcurves.is_empty() {
                return None;
            }
            for (geometry, range) in &item.pcurves {
                append_path(
                    &mut parameter_points,
                    linear_pcurve_points(geometry, *range)?,
                )?;
            }
        }
        normalize_parameter_ring_endpoints(&mut parameter_points, closure_tolerance);
        Some(LinearBoundaryGeometry::Parameter(parameter_points))
    } else {
        Some(LinearBoundaryGeometry::Model(model_coordinates))
    }
}

fn linear_ring_is_simple(points: &[[f64; 2]]) -> bool {
    let Some(last) = points.len().checked_sub(1) else {
        return false;
    };
    if points.len() < 4
        || points.first() != points.last()
        || points
            .iter()
            .flatten()
            .any(|coordinate| !coordinate.is_finite())
    {
        return false;
    }
    if points.windows(2).any(|segment| segment[0] == segment[1]) {
        return false;
    }
    for first in 0..last {
        for second in first + 1..last {
            if points[first] == points[second] {
                return false;
            }
        }
    }
    !planar_polyline_has_self_intersection(points)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PlanarPointLocation {
    Inside,
    Boundary,
    Outside,
}

fn planar_point_location(point: [f64; 2], ring: &[[f64; 2]]) -> PlanarPointLocation {
    if ring.windows(2).any(|segment| {
        super::geometry::planar_segments_contain_point(point, [segment[0], segment[1]])
    }) {
        return PlanarPointLocation::Boundary;
    }
    let mut inside = false;
    for segment in ring.windows(2) {
        let [left, right] = [segment[0], segment[1]];
        if (left[1] > point[1]) != (right[1] > point[1]) {
            let crossing =
                left[0] + (right[0] - left[0]) * (point[1] - left[1]) / (right[1] - left[1]);
            if point[0] < crossing {
                inside = !inside;
            }
        }
    }
    if inside {
        PlanarPointLocation::Inside
    } else {
        PlanarPointLocation::Outside
    }
}

fn linear_boundary_rings(
    candidates: &[Option<LinearBoundaryGeometry>],
    parameter: bool,
) -> Option<Vec<Vec<[f64; 2]>>> {
    if candidates.is_empty() {
        return None;
    }
    candidates
        .iter()
        .map(|candidate| match (parameter, candidate.as_ref()) {
            (true, Some(LinearBoundaryGeometry::Parameter(points)))
            | (false, Some(LinearBoundaryGeometry::Model(points))) => Some(points.clone()),
            _ => None,
        })
        .collect()
}

fn inner_boundaries_are_disjoint_and_inside(outer: &[[f64; 2]], inners: &[Vec<[f64; 2]>]) -> bool {
    for inner in inners {
        if planar_polylines_intersect(outer, inner)
            || inner[..inner.len() - 1]
                .iter()
                .any(|point| planar_point_location(*point, outer) != PlanarPointLocation::Inside)
        {
            return false;
        }
    }
    inners.iter().enumerate().all(|(left_index, left)| {
        inners.iter().skip(left_index + 1).all(|right| {
            !planar_polylines_intersect(left, right)
                && planar_point_location(left[0], right) != PlanarPointLocation::Inside
                && planar_point_location(right[0], left) != PlanarPointLocation::Inside
        })
    })
}

fn linear_boundary_relationship_is_valid(
    rings: &[Vec<[f64; 2]>],
    trimmed_surface: bool,
    has_explicit_outer: bool,
    support: &SurfaceGeometry,
    support_bounds: Option<[Option<f64>; 4]>,
    periodic_parameters: [bool; 2],
) -> Option<bool> {
    if !rings.iter().all(|ring| linear_ring_is_simple(ring)) {
        return Some(false);
    }
    if !trimmed_surface {
        return Some(true);
    }
    if has_explicit_outer {
        let (outer, inners) = rings.split_first()?;
        return Some(inner_boundaries_are_disjoint_and_inside(outer, inners));
    }
    if periodic_parameters.iter().any(|periodic| *periodic) {
        return None;
    }
    match support_bounds {
        Some([Some(u_lower), Some(u_upper), Some(v_lower), Some(v_upper)])
            if u_lower.is_finite()
                && u_upper.is_finite()
                && v_lower.is_finite()
                && v_upper.is_finite()
                && u_lower < u_upper
                && v_lower < v_upper =>
        {
            if rings.iter().any(|ring| {
                ring[..ring.len() - 1].iter().any(|point| {
                    point[0] <= u_lower
                        || point[0] >= u_upper
                        || point[1] <= v_lower
                        || point[1] >= v_upper
                })
            }) {
                return Some(false);
            }
        }
        Some(_) => return None,
        None if !matches!(support, SurfaceGeometry::Plane { .. }) => return None,
        None => {}
    }
    Some(rings.iter().enumerate().all(|(left_index, left)| {
        rings.iter().skip(left_index + 1).all(|right| {
            !planar_polylines_intersect(left, right)
                && planar_point_location(left[0], right) != PlanarPointLocation::Inside
                && planar_point_location(right[0], left) != PlanarPointLocation::Inside
        })
    }))
}

#[derive(Clone)]
struct HomogeneousPcurveSpan {
    domain: [f64; 2],
    controls: Vec<[f64; 4]>,
}

type HomogeneousPcurveSplit = (Vec<[f64; 4]>, Vec<[f64; 4]>);

fn insert_homogeneous_pcurve_knot(
    degree: usize,
    knots: &mut Vec<f64>,
    controls: &mut Vec<[f64; 4]>,
    knot: f64,
) -> Option<()> {
    let count = controls.len();
    let span = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1])?;
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Some(());
    }
    let left_end = span.checked_sub(degree)?;
    let tail_start = span.checked_sub(multiplicity)?;
    let mut inserted = (0..count.checked_add(1)?)
        .map(|_| [0.0; 4])
        .collect::<Vec<_>>();
    inserted[..=left_end].copy_from_slice(&controls[..=left_end]);
    inserted[tail_start + 1..].copy_from_slice(&controls[tail_start..]);
    for index in left_end + 1..=tail_start {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return None;
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    knots.insert(span + 1, knot);
    *controls = inserted;
    Some(())
}

fn homogeneous_pcurve_spans(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; 4]>,
) -> Option<Vec<HomogeneousPcurveSpan>> {
    if degree == 0
        || degree >= controls.len()
        || knots.len() != controls.len().checked_add(degree)?.checked_add(1)?
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return None;
    }
    let domain = [*knots.get(degree)?, *knots.get(controls.len())?];
    if domain[0] >= domain[1] {
        return None;
    }
    let mut knots = knots.to_vec();
    let mut internal = knots
        .get(degree + 1..controls.len())?
        .iter()
        .copied()
        .filter(|knot| domain[0] < *knot && *knot < domain[1])
        .collect::<Vec<_>>();
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            insert_homogeneous_pcurve_knot(degree, &mut knots, &mut controls, knot)?;
        }
    }
    let mut spans = Vec::new();
    for span in degree..controls.len() {
        let &start = knots.get(span)?;
        let &end = knots.get(span + 1)?;
        if start >= end {
            continue;
        }
        spans.push(HomogeneousPcurveSpan {
            domain: [start, end],
            controls: controls.get(span.checked_sub(degree)?..=span)?.to_vec(),
        });
    }
    (!spans.is_empty()).then_some(spans)
}

fn split_homogeneous_pcurve(
    controls: &[[f64; 4]],
    parameter: f64,
) -> Option<HomogeneousPcurveSplit> {
    if controls.is_empty() || !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
        return None;
    }
    let mut levels = vec![controls.to_vec()];
    while levels.last()?.len() > 1 {
        let next = levels
            .last()?
            .windows(2)
            .map(|pair| {
                std::array::from_fn(|axis| {
                    (1.0 - parameter) * pair[0][axis] + parameter * pair[1][axis]
                })
            })
            .collect::<Vec<_>>();
        levels.push(next);
    }
    let left = levels.iter().map(|level| level[0]).collect::<Vec<_>>();
    let right = levels
        .iter()
        .rev()
        .map(|level| *level.last().expect("nonempty de Casteljau level"))
        .collect::<Vec<_>>();
    Some((left, right))
}

fn restrict_homogeneous_pcurve(
    controls: &[[f64; 4]],
    start: f64,
    end: f64,
) -> Option<Vec<[f64; 4]>> {
    if start > end {
        let mut restricted = restrict_homogeneous_pcurve(controls, end, start)?;
        restricted.reverse();
        return Some(restricted);
    }
    if start == end {
        let point = split_homogeneous_pcurve(controls, start)?
            .0
            .into_iter()
            .last()?;
        return Some(std::iter::once(point).collect());
    }
    let left = split_homogeneous_pcurve(controls, end)?.0;
    if start == 0.0 {
        return Some(left);
    }
    let relative_start = start / end;
    split_homogeneous_pcurve(&left, relative_start).map(|(_, right)| right)
}

fn pcurve_within_declared_bounds(
    geometry: &PcurveGeometry,
    range: [f64; 2],
    bounds: Option<[Option<f64>; 4]>,
    periodic: [bool; 2],
) -> bool {
    let Some(bounds) = bounds else {
        return true;
    };
    let PcurveGeometry::Nurbs {
        degree,
        knots,
        control_points,
        weights,
        ..
    } = geometry
    else {
        return false;
    };
    let Some(degree) = usize::try_from(*degree).ok() else {
        return false;
    };
    if !range[0].is_finite() || !range[1].is_finite() || range[0] >= range[1] {
        return false;
    }
    if weights
        .as_ref()
        .is_some_and(|weights| weights.len() != control_points.len())
    {
        return false;
    }
    let Some(controls) = control_points
        .iter()
        .enumerate()
        .map(|(index, point)| {
            let weight = weights
                .as_ref()
                .map_or(Some(1.0), |weights| weights.get(index).copied())?;
            (weight.is_finite() && weight > 0.0).then_some([
                weight,
                weight * point.u,
                weight * point.v,
                0.0,
            ])
        })
        .collect::<Option<Vec<_>>>()
    else {
        return false;
    };
    let Some(spans) = homogeneous_pcurve_spans(degree, knots, controls) else {
        return false;
    };
    let Some(first_span) = spans.first() else {
        return false;
    };
    let Some(last_span) = spans.last() else {
        return false;
    };
    if range[0] < first_span.domain[0] || range[1] > last_span.domain[1] {
        return false;
    }
    let in_bound = |value: f64, lower: Option<f64>, upper: Option<f64>| {
        value.is_finite()
            && lower.is_none_or(|lower| lower.is_finite() && value >= lower)
            && upper.is_none_or(|upper| upper.is_finite() && value <= upper)
    };
    let expand_periodic =
        |lower: Option<f64>, upper: Option<f64>, periodic: bool| match (lower, upper, periodic) {
            (Some(lower), Some(upper), true) => {
                let period = upper - lower;
                (period.is_finite() && period > 0.0)
                    .then_some((Some(lower - period), Some(upper + period)))
            }
            _ => Some((lower, upper)),
        };
    let Some((u_lower, u_upper)) = expand_periodic(bounds[0], bounds[1], periodic[0]) else {
        return false;
    };
    let Some((v_lower, v_upper)) = expand_periodic(bounds[2], bounds[3], periodic[1]) else {
        return false;
    };
    let mut covered = false;
    for span in spans {
        let start = range[0].max(span.domain[0]);
        let end = range[1].min(span.domain[1]);
        if start >= end {
            continue;
        }
        covered = true;
        let width = span.domain[1] - span.domain[0];
        if !width.is_finite() || width <= 0.0 {
            return false;
        }
        let local_start = (start - span.domain[0]) / width;
        let local_end = (end - span.domain[0]) / width;
        let Some(restricted) = restrict_homogeneous_pcurve(&span.controls, local_start, local_end)
        else {
            return false;
        };
        if restricted.iter().any(|control| {
            let weight = control[0];
            !weight.is_finite()
                || weight <= 0.0
                || !in_bound(control[1] / weight, u_lower, u_upper)
                || !in_bound(control[2] / weight, v_lower, v_upper)
        }) {
            return false;
        }
    }
    covered
}

fn periodic_surface_parameters(surface: &SurfaceGeometry) -> [bool; 2] {
    match surface {
        SurfaceGeometry::Nurbs(surface) => [surface.u_periodic, surface.v_periodic],
        _ => [false, false],
    }
}

fn surface_parameter_bounds(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
) -> Option<[Option<f64>; 4]> {
    fn visit(
        index: &ModelIndex<'_>,
        surface_id: &SurfaceId,
        visiting: &mut BTreeSet<SurfaceId>,
    ) -> Option<[Option<f64>; 4]> {
        if !visiting.insert(surface_id.clone()) {
            return None;
        }
        let procedural = index.procedural_surface_for_surface(&surface_id.0)?;
        let bounds = match &procedural.definition {
            ProceduralSurfaceDefinition::Ruled { .. }
            | ProceduralSurfaceDefinition::Extrusion { .. } => procedural
                .record_bounds
                .map(|bounds| [bounds[0], bounds[1], Some(0.0), Some(1.0)]),
            ProceduralSurfaceDefinition::Revolution {
                angular_interval, ..
            } => procedural.record_bounds.map(|bounds| {
                [
                    bounds[0],
                    bounds[1],
                    Some(angular_interval[0]),
                    Some(angular_interval[1]),
                ]
            }),
            _ => procedural.record_bounds,
        };
        if let Some(bounds) = bounds {
            return Some(bounds);
        }
        let support = match &procedural.definition {
            ProceduralSurfaceDefinition::Offset { support, .. }
            | ProceduralSurfaceDefinition::ParallelOffset { support, .. } => support,
            ProceduralSurfaceDefinition::Replica { source, .. } => source,
            _ => return None,
        };
        visit(index, support, visiting)
    }

    visit(index, surface_id, &mut BTreeSet::new())
}

fn pcurves_agree(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    pcurves: &[(PcurveGeometry, [f64; 2])],
    expected_start: Point3,
    expected_end: Point3,
    tolerance: f64,
) -> bool {
    let mapped = pcurves
        .iter()
        .map(|(geometry, range)| {
            let start = evaluation::pcurve(geometry, range[0]).and_then(|uv| {
                cadmpeg_ir::eval::model_surface_point_by_id(index, surface_id, uv.u, uv.v)
            })?;
            let end = evaluation::pcurve(geometry, range[1]).and_then(|uv| {
                cadmpeg_ir::eval::model_surface_point_by_id(index, surface_id, uv.u, uv.v)
            })?;
            Some((start, end))
        })
        .collect::<Option<Vec<_>>>();
    let Some(mapped) = mapped else {
        return false;
    };
    mapped
        .first()
        .is_some_and(|(start, _)| close(*start, expected_start, tolerance))
        && mapped
            .last()
            .is_some_and(|(_, end)| close(*end, expected_end, tolerance))
        && mapped
            .windows(2)
            .all(|pair| close(pair[0].1, pair[1].0, tolerance))
}

fn edge_range_matches_curve(
    edge: &Edge,
    carrier_index: &ModelIndex<'_>,
    start: Point3,
    end: Point3,
    tolerance: f64,
) -> bool {
    let Some(curve_id) = edge.curve.as_ref() else {
        return false;
    };
    let Some(curve) = carrier_index.curves(&curve_id.0) else {
        return false;
    };
    let Some(range) = edge.param_range else {
        return false;
    };
    if !range.iter().all(|parameter| parameter.is_finite()) {
        return false;
    }
    let Some(evaluated_start) = cadmpeg_ir::eval::curve_point(&curve.geometry, range[0]) else {
        return false;
    };
    let Some(evaluated_end) = cadmpeg_ir::eval::curve_point(&curve.geometry, range[1]) else {
        return false;
    };
    close(evaluated_start, start, tolerance) && close(evaluated_end, end, tolerance)
}

fn select_boundary_edge(
    candidates: &[Edge],
    carrier_index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    pcurves: &[(PcurveGeometry, [f64; 2])],
    sense: Sense,
    tolerance: f64,
    parameter_curves_authoritative: bool,
) -> Result<(Edge, Point3, Point3, bool), BoundaryEdgeSelectionError> {
    let mut candidates_with_endpoints = 0;
    let candidates = candidates
        .iter()
        .filter_map(|edge| {
            let start = point_position(carrier_index, &edge.start)?;
            let end = point_position(carrier_index, &edge.end)?;
            candidates_with_endpoints += 1;
            edge_range_matches_curve(edge, carrier_index, start, end, tolerance)
                .then_some((edge, start, end))
        })
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return Err(if candidates_with_endpoints == 0 {
            BoundaryEdgeSelectionError::MissingEndpoints
        } else {
            BoundaryEdgeSelectionError::InvalidRange
        });
    }
    if pcurves.is_empty() {
        return if candidates.len() == 1 {
            let (edge, start, end) = candidates[0];
            Ok((edge.clone(), start, end, true))
        } else {
            Err(BoundaryEdgeSelectionError::Ambiguous)
        };
    }

    let agreeing = candidates
        .iter()
        .filter(|(_, start, end)| {
            let (expected_start, expected_end) = if sense == Sense::Forward {
                (*start, *end)
            } else {
                (*end, *start)
            };
            pcurves_agree(
                carrier_index,
                surface_id,
                pcurves,
                expected_start,
                expected_end,
                tolerance,
            )
        })
        .collect::<Vec<_>>();
    match agreeing.as_slice() {
        [(edge, start, end)] => Ok(((*edge).clone(), *start, *end, true)),
        [] if !parameter_curves_authoritative && candidates.len() == 1 => {
            let (edge, start, end) = candidates[0];
            Ok((edge.clone(), start, end, false))
        }
        [] => {
            if parameter_curves_authoritative {
                Err(BoundaryEdgeSelectionError::PcurveDisagreement)
            } else {
                Err(BoundaryEdgeSelectionError::Ambiguous)
            }
        }
        _ => Err(BoundaryEdgeSelectionError::Ambiguous),
    }
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: Option<&DecodeContext<'_>>,
) -> (ProjectionOutcome, Vec<BoundaryVertexDerivation>) {
    let records = parameters
        .iter()
        .map(|record| (record.directory_sequence, record))
        .collect::<BTreeMap<_, _>>();
    let entries = directory
        .iter()
        .map(|entry| (entry.sequence, entry))
        .collect::<BTreeMap<_, _>>();
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut boundary_vertex_derivations = Vec::new();
    let mut boundaries = BTreeMap::new();

    let carrier_index = ModelIndex::new(ir);
    let mut composite_index: Option<CompositeIndex> = None;
    let mut edges_by_curve = BTreeMap::<CurveId, Vec<Edge>>::new();
    for edge in &ir.model.edges {
        if let Some(curve) = &edge.curve {
            edges_by_curve
                .entry(curve.clone())
                .or_default()
                .push(edge.clone());
        }
    }
    let mut staged = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 142 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        if !matches!(record.integer(1), Some(0..=3)) || !matches!(record.integer(5), Some(0..=3)) {
            losses.push(entity_loss(
                entry,
                "curve-on-surface creation or preference flag is invalid",
            ));
            continue;
        }
        let preference = record.integer(5).expect("validated preference flag");
        let Some(surface) = pointer(record, 2) else {
            losses.push(entity_loss(
                entry,
                "curve-on-surface surface pointer is invalid",
            ));
            continue;
        };
        let pcurve = match record.integer(3) {
            Some(0) => None,
            Some(value) => u32::try_from(value)
                .ok()
                .filter(|sequence| sequence % 2 == 1),
            None => None,
        };
        if record
            .integer(3)
            .is_none_or(|value| value != 0 && pcurve.is_none())
        {
            losses.push(entity_loss(
                entry,
                "curve-on-surface parameter curve pointer is invalid",
            ));
            continue;
        }
        let Some(model_curve) = pointer(record, 4) else {
            losses.push(entity_loss(
                entry,
                "curve-on-surface model curve pointer is invalid",
            ));
            continue;
        };
        if pcurve.is_some_and(|pcurve| {
            entries
                .get(&pcurve)
                .is_none_or(|entry| entry.status.use_flag != 5)
        }) {
            losses.push(entity_loss(
                entry,
                "parameter curve does not have entity-use flag 05",
            ));
            continue;
        }
        boundaries.insert(
            entry.sequence,
            BoundaryDefinition {
                surface,
                segments: vec![BoundarySegment {
                    pcurves: pcurve.into_iter().collect(),
                    model_curve,
                    sense: Sense::Forward,
                    parameter_curves_authoritative: pcurve.is_some() && preference != 2,
                }],
            },
        );
        decoded.insert(entry.sequence);
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 141 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let Some(boundary_type) = record.integer(1).filter(|value| matches!(value, 0 | 1)) else {
            losses.push(entity_loss(
                entry,
                "boundary representation type is not 0 or 1",
            ));
            continue;
        };
        if !matches!(record.integer(2), Some(0..=3)) {
            losses.push(entity_loss(entry, "boundary preference flag is invalid"));
            continue;
        }
        let preference = record.integer(2).expect("validated preference flag");
        let Some(surface) = pointer(record, 3) else {
            losses.push(entity_loss(entry, "boundary support pointer is invalid"));
            continue;
        };
        let Some(segment_count) = record.count(4).filter(|count| *count > 0) else {
            losses.push(entity_loss(entry, "boundary segment count is not positive"));
            continue;
        };
        let mut index = 5;
        let mut segments = Vec::with_capacity(segment_count);
        let mut valid = true;
        for _ in 0..segment_count {
            let Some(model_curve) = pointer(record, index) else {
                losses.push(entity_loss(
                    entry,
                    "boundary model-curve pointer is invalid",
                ));
                valid = false;
                break;
            };
            let sense = match record.integer(index + 1) {
                Some(1) => Sense::Forward,
                Some(2) => Sense::Reversed,
                _ => {
                    losses.push(entity_loss(entry, "boundary segment sense is not 1 or 2"));
                    valid = false;
                    break;
                }
            };
            let Some(pcurve_count) = record.count(index + 2) else {
                losses.push(entity_loss(entry, "boundary pcurve count is invalid"));
                valid = false;
                break;
            };
            if (boundary_type == 0 && pcurve_count != 0)
                || (boundary_type == 1 && pcurve_count == 0)
            {
                losses.push(entity_loss(
                    entry,
                    "boundary pcurve collection cardinality disagrees with its representation type",
                ));
                valid = false;
                break;
            }
            let mut pcurves = Vec::with_capacity(pcurve_count);
            for pcurve_index in 0..pcurve_count {
                let Some(pcurve) = pointer(record, index + 3 + pcurve_index) else {
                    pcurves.clear();
                    break;
                };
                if entries
                    .get(&pcurve)
                    .is_none_or(|entry| entry.status.use_flag != 5)
                {
                    losses.push(entity_loss(
                        entry,
                        "boundary pcurve does not have entity-use flag 05",
                    ));
                    pcurves.clear();
                    break;
                }
                pcurves.push(pcurve);
            }
            if pcurves.len() != pcurve_count {
                losses.push(entity_loss(entry, "boundary pcurve pointer is invalid"));
                valid = false;
                break;
            }
            segments.push(BoundarySegment {
                model_curve,
                pcurves,
                sense,
                parameter_curves_authoritative: preference != 1,
            });
            index += 3 + pcurve_count;
        }
        if valid {
            boundaries.insert(entry.sequence, BoundaryDefinition { surface, segments });
            decoded.insert(entry.sequence);
        }
    }
    for entry in directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 143 | 144) && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let carrier_agreement_tolerance = global.minimum_resolution_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            losses.push(entity_loss(entry, "Parameter Data record is missing"));
            continue;
        };
        let trimmed_surface = entry.entity_type == 144;
        let (surface_sequence, boundary_sequences, has_explicit_outer, mut valid) =
            if trimmed_surface {
                let Some(surface) = pointer(record, 1) else {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface support pointer is invalid",
                    ));
                    continue;
                };
                let Some(has_explicit_outer) = record.integer(2).and_then(|value| match value {
                    0 => Some(false),
                    1 => Some(true),
                    _ => None,
                }) else {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface outer-boundary flag is not 0 or 1",
                    ));
                    continue;
                };
                let Some(inner_count) = record.count(3) else {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface inner-boundary count is invalid",
                    ));
                    continue;
                };
                let mut sequences =
                    Vec::with_capacity(inner_count + usize::from(has_explicit_outer));
                if has_explicit_outer {
                    let Some(outer) = pointer(record, 4) else {
                        losses.push(entity_loss(
                            entry,
                            "trimmed-surface outer-boundary pointer is invalid",
                        ));
                        continue;
                    };
                    if entries
                        .get(&outer)
                        .is_none_or(|target| target.entity_type != 142 || target.form != 0)
                    {
                        losses.push(entity_loss(
                            entry,
                            "trimmed-surface outer-boundary pointer does not target a Type 142 Form 0 entity",
                        ));
                        continue;
                    }
                    sequences.push(outer);
                } else if !matches!(
                    record.value(4),
                    None | Some(TokenValue::Omitted | TokenValue::Integer(0))
                ) {
                    losses.push(entity_loss(
                        entry,
                        "trimmed-surface parameter-domain outer-boundary pointer is neither zero nor omitted",
                    ));
                    continue;
                }
                let mut valid = true;
                for index in 0..inner_count {
                    let Some(sequence) = pointer(record, 5 + index) else {
                        losses.push(entity_loss(
                            entry,
                            "trimmed-surface inner-boundary pointer is invalid",
                        ));
                        valid = false;
                        break;
                    };
                    if entries
                        .get(&sequence)
                        .is_none_or(|target| target.entity_type != 142 || target.form != 0)
                    {
                        losses.push(entity_loss(
                            entry,
                            "trimmed-surface inner-boundary pointer does not target a Type 142 Form 0 entity",
                        ));
                        valid = false;
                        break;
                    }
                    sequences.push(sequence);
                }
                (surface, sequences, has_explicit_outer, valid)
            } else {
                let Some(representation) = record.integer(1).filter(|value| matches!(value, 0 | 1))
                else {
                    losses.push(entity_loss(
                        entry,
                        "bounded-surface representation type is not 0 or 1",
                    ));
                    continue;
                };
                let Some(surface) = pointer(record, 2) else {
                    losses.push(entity_loss(
                        entry,
                        "bounded-surface support pointer is invalid",
                    ));
                    continue;
                };
                let Some(count) = record.count(3).filter(|count| *count > 0) else {
                    losses.push(entity_loss(
                        entry,
                        "bounded-surface boundary count is not positive",
                    ));
                    continue;
                };
                let mut sequences = Vec::with_capacity(count);
                let mut valid = true;
                for index in 0..count {
                    let Some(sequence) = pointer(record, 4 + index) else {
                        losses.push(entity_loss(
                            entry,
                            "bounded-surface boundary pointer is invalid",
                        ));
                        valid = false;
                        break;
                    };
                    if entries
                        .get(&sequence)
                        .is_none_or(|target| target.entity_type != 141 || target.form != 0)
                    {
                        losses.push(entity_loss(
                            entry,
                            "bounded-surface boundary pointer does not target a Type 141 Form 0 entity",
                        ));
                        valid = false;
                        break;
                    }
                    if boundaries.get(&sequence).is_some_and(|boundary| {
                        (representation == 0
                            && boundary
                                .segments
                                .iter()
                                .all(|segment| segment.pcurves.is_empty()))
                            || (representation == 1
                                && boundary
                                    .segments
                                    .iter()
                                    .all(|segment| !segment.pcurves.is_empty()))
                    }) {
                        sequences.push(sequence);
                    } else {
                        losses.push(entity_loss(
                            entry,
                            "bounded-surface representation disagrees with its boundary",
                        ));
                        valid = false;
                        break;
                    }
                }
                (surface, sequences, false, valid)
            };
        if !valid {
            continue;
        }
        let surface_id = SurfaceId(format!("iges:model:surface#D{surface_sequence}"));
        let Some(support_geometry) = carrier_index
            .surfaces(&surface_id.0)
            .map(|surface| surface.geometry.clone())
        else {
            losses.push(entity_loss(
                entry,
                "trimmed-surface support carrier is missing",
            ));
            continue;
        };
        let mut candidate = ModelDraft::new();
        let stem = format!("D{}", entry.sequence);
        let body_id = BodyId(format!("iges:model:body#{stem}"));
        let region_id = RegionId(format!("iges:model:region#{stem}"));
        let shell_id = ShellId(format!("iges:model:shell#{stem}"));
        let face_id = FaceId(format!("iges:model:face#{stem}"));
        let mut candidate_boundary_vertex_derivations = Vec::new();
        let support_parameter_bounds = surface_parameter_bounds(&carrier_index, &surface_id);
        let periodic_parameters = periodic_surface_parameters(&support_geometry);
        let implicit_outer_domain =
            trimmed_surface && !has_explicit_outer && !boundary_sequences.is_empty();
        let mut implicit_boundary_curves = Vec::new();
        let mut implicit_boundary_pcurves = Vec::new();
        let mut loop_ids = Vec::new();
        let mut linear_boundary_candidates = Vec::with_capacity(boundary_sequences.len());
        let mut face_tolerance = 0.0_f64;
        for (boundary_index, sequence) in boundary_sequences.iter().copied().enumerate() {
            let Some(boundary) = boundaries.get(&sequence).cloned() else {
                losses.push(entity_loss(
                    entry,
                    "trimmed-surface boundary definition is missing",
                ));
                valid = false;
                break;
            };
            if boundary.surface != surface_sequence {
                losses.push(entity_loss(
                    entry,
                    "boundary definition names a different support surface",
                ));
                valid = false;
                break;
            }
            let mut items = Vec::with_capacity(boundary.segments.len());
            for segment in &boundary.segments {
                let model_curve_id = CurveId(format!("iges:model:curve#D{}", segment.model_curve));
                let Some(candidates) = edges_by_curve.get(&model_curve_id) else {
                    losses.push(entity_loss(
                        entry,
                        "boundary model curve has no bounded edge",
                    ));
                    valid = false;
                    break;
                };
                let pcurves = segment
                    .pcurves
                    .iter()
                    .map(|sequence| {
                        pcurve_geometry(
                            ir,
                            *sequence,
                            &PcurveSupport {
                                surface_id: &surface_id,
                                geometry: &support_geometry,
                                factor,
                            },
                            Some(carrier_agreement_tolerance),
                            ctx,
                            Some(
                                composite_index.get_or_insert_with(|| CompositeIndex::from_ir(ir)),
                            ),
                        )
                    })
                    .collect::<Option<Vec<_>>>();
                let mut pcurves = match pcurves {
                    Some(pcurves) => pcurves,
                    None if segment.parameter_curves_authoritative => {
                        losses.push(entity_loss(
                            entry,
                            "boundary parameter curve has no NURBS carrier",
                        ));
                        valid = false;
                        break;
                    }
                    None => Vec::new(),
                };
                if pcurves.iter().any(|(geometry, range)| {
                    !pcurve_within_declared_bounds(
                        geometry,
                        *range,
                        support_parameter_bounds,
                        periodic_parameters,
                    )
                }) {
                    if segment.parameter_curves_authoritative {
                        losses.push(boundary_parameter_loss(
                            entry,
                            "boundary parameter curve leaves the declared support parameter bounds",
                        ));
                        valid = false;
                        break;
                    }
                    losses.push(boundary_parameter_loss(
                        entry,
                        "alternate boundary parameter curve leaves the declared support parameter bounds; model-space curve retained",
                    ));
                    pcurves.clear();
                }
                let (source_edge, start, end, pcurves_agree) = match select_boundary_edge(
                    candidates,
                    &carrier_index,
                    &surface_id,
                    &pcurves,
                    segment.sense,
                    carrier_agreement_tolerance,
                    segment.parameter_curves_authoritative,
                ) {
                    Ok(selected) => selected,
                    Err(BoundaryEdgeSelectionError::MissingEndpoints) => {
                        losses.push(entity_loss(
                            entry,
                            "boundary model-curve endpoints are missing",
                        ));
                        valid = false;
                        break;
                    }
                    Err(BoundaryEdgeSelectionError::InvalidRange) => {
                        losses.push(entity_loss(
                            entry,
                            "boundary model-curve edge range does not evaluate to its vertices",
                        ));
                        valid = false;
                        break;
                    }
                    Err(BoundaryEdgeSelectionError::Ambiguous) => {
                        losses.push(entity_loss(
                            entry,
                            "boundary model curve maps to multiple ambiguous edge occurrences",
                        ));
                        valid = false;
                        break;
                    }
                    Err(BoundaryEdgeSelectionError::PcurveDisagreement) => {
                        losses.push(entity_loss(
                            entry,
                            "curve-on-surface carriers disagree beyond the minimum resolution",
                        ));
                        valid = false;
                        break;
                    }
                };
                if !pcurves_agree {
                    pcurves.clear();
                }
                items.push(BoundaryItem {
                    segment: segment.clone(),
                    model_curve: model_curve_id,
                    source_edge,
                    start,
                    end,
                    pcurves,
                });
            }
            if !valid {
                break;
            }
            if implicit_outer_domain {
                implicit_boundary_curves.extend(items.iter().map(|item| item.model_curve.clone()));
            }
            let traversal = |item: &BoundaryItem| {
                if item.segment.sense == Sense::Forward {
                    (item.start, item.end)
                } else {
                    (item.end, item.start)
                }
            };
            let tolerance_policy = FaceTolerancePolicy::from_global(
                global,
                items.iter().flat_map(|item| [item.start, item.end]),
            );
            let sewing_tolerance = tolerance_policy.topology_sewing;
            face_tolerance = face_tolerance.max(sewing_tolerance);
            if items.iter().enumerate().any(|(index, item)| {
                let (_, end) = traversal(item);
                let (next_start, _) = traversal(&items[(index + 1) % items.len()]);
                !close(end, next_start, sewing_tolerance)
            }) {
                losses.push(entity_loss(
                    entry,
                    "ordered boundary segments do not form a closed ring",
                ));
                valid = false;
                break;
            }
            linear_boundary_candidates.push(linear_boundary_geometry(
                &items,
                &carrier_index,
                &support_geometry,
                carrier_agreement_tolerance,
                sewing_tolerance,
                trimmed_surface,
            ));
            let loop_id = LoopId(format!("iges:model:loop#{stem}:{boundary_index}"));
            let coedge_ids = (0..items.len())
                .map(|index| CoedgeId(format!("iges:model:coedge#{stem}:{boundary_index}:{index}")))
                .collect::<Vec<_>>();
            let source_endpoints = items
                .iter()
                .flat_map(|item| {
                    [
                        BoundaryVertexSourceEndpoint {
                            edge: item.source_edge.id.0.clone(),
                            endpoint: BoundaryEndpoint::Start,
                            position: item.start,
                        },
                        BoundaryVertexSourceEndpoint {
                            edge: item.source_edge.id.0.clone(),
                            endpoint: BoundaryEndpoint::End,
                            position: item.end,
                        },
                    ]
                })
                .collect::<Vec<_>>();
            let (vertex_ids, derivations) = match create_boundary_vertices(
                &mut candidate,
                &stem,
                &format!("iges:entity:directory#{}", entry.sequence),
                boundary_index,
                &source_endpoints,
                sewing_tolerance,
            ) {
                Ok(result) => result,
                Err(BoundaryVertexClusterError::InvalidTolerance) => {
                    losses.push(entity_loss(entry, "boundary sewing tolerance is invalid"));
                    valid = false;
                    break;
                }
                Err(BoundaryVertexClusterError::NonTransitive) => {
                    losses.push(entity_loss(
                        entry,
                        "boundary endpoint tolerance neighborhoods are non-transitive",
                    ));
                    valid = false;
                    break;
                }
            };
            candidate_boundary_vertex_derivations.extend(derivations);
            for (segment_index, item) in items.into_iter().enumerate() {
                let edge_id = EdgeId(format!(
                    "iges:model:edge#{stem}:{boundary_index}:{segment_index}"
                ));
                let start_vertex = vertex_ids[segment_index * 2].clone();
                let end_vertex = vertex_ids[segment_index * 2 + 1].clone();
                candidate.model_mut().edges.push(Edge {
                    id: edge_id.clone(),
                    curve: Some(item.model_curve),
                    start: start_vertex,
                    end: end_vertex,
                    param_range: item.source_edge.param_range,
                    tolerance: Some(sewing_tolerance),
                });
                let pcurve_uses = item
                    .pcurves
                    .into_iter()
                    .enumerate()
                    .map(|(pcurve_index, (geometry, parameter_range))| {
                        let id = PcurveId(format!(
                            "iges:model:pcurve#{stem}:{boundary_index}:{segment_index}:{pcurve_index}"
                        ));
                        if implicit_outer_domain {
                            implicit_boundary_pcurves.push(id.clone());
                        }
                        candidate.model_mut().pcurves.push(Pcurve {
                            id: id.clone(),
                            geometry,
                            wrapper_reversed: None,
                            native_tail_flags: None,
                            parameter_range: Some(parameter_range),
                            fit_tolerance: None,
                        });
                        PcurveUse {
                            pcurve: id,
                            isoparametric: None,
                            parameter_range: None,
                        }
                    })
                    .collect();
                let coedge_id = coedge_ids[segment_index].clone();
                candidate.model_mut().coedges.push(Coedge {
                    id: coedge_id.clone(),
                    owner_loop: loop_id.clone(),
                    edge: edge_id,
                    next: coedge_ids[(segment_index + 1) % coedge_ids.len()].clone(),
                    previous: coedge_ids[(segment_index + coedge_ids.len() - 1) % coedge_ids.len()]
                        .clone(),
                    radial_next: coedge_id,
                    sense: item.segment.sense,
                    pcurves: pcurve_uses,
                    use_curve: None,
                    use_curve_parameter_range: None,
                });
            }
            candidate.model_mut().loops.push(Loop {
                id: loop_id.clone(),
                face: face_id.clone(),
                boundary_role: if trimmed_surface {
                    if has_explicit_outer && boundary_index == 0 {
                        cadmpeg_ir::topology::LoopBoundaryRole::Outer
                    } else {
                        cadmpeg_ir::topology::LoopBoundaryRole::Inner
                    }
                } else {
                    cadmpeg_ir::topology::LoopBoundaryRole::Unspecified
                },
                coedges: coedge_ids,
                vertex_uses: Vec::new(),
            });
            loop_ids.push(loop_id);
        }
        if !valid {
            continue;
        }
        let linear_rings = linear_boundary_rings(&linear_boundary_candidates, true)
            .or_else(|| linear_boundary_rings(&linear_boundary_candidates, false));
        if let Some(rings) = linear_rings {
            if linear_boundary_relationship_is_valid(
                &rings,
                trimmed_surface,
                has_explicit_outer,
                &support_geometry,
                support_parameter_bounds,
                periodic_parameters,
            ) == Some(false)
            {
                losses.push(entity_loss(
                    entry,
                    if trimmed_surface {
                        "trimmed-surface boundary loops are not simple, disjoint, and correctly nested"
                    } else {
                        "boundary loop is not a simple closed carrier"
                    },
                ));
                continue;
            }
        }
        let face_surface_id = if implicit_outer_domain {
            let derived_surface_id = SurfaceId(format!(
                "iges:model:surface#D{}:implicit-outer",
                entry.sequence
            ));
            candidate.model_mut().surfaces.push(Surface {
                id: derived_surface_id.clone(),
                geometry: support_geometry.clone(),
                source_object: Some(source_object(entry)),
            });
            candidate
                .model_mut()
                .procedural_surfaces
                .push(ProceduralSurface {
                    id: ProceduralSurfaceId(format!(
                        "iges:model:procedural-surface#D{}:implicit-outer",
                        entry.sequence
                    )),
                    surface: derived_surface_id.clone(),
                    definition: ProceduralSurfaceDefinition::CurveBounded {
                        support: surface_id.clone(),
                        boundaries: implicit_boundary_curves,
                        boundary_pcurves: implicit_boundary_pcurves,
                        implicit_outer: true,
                    },
                    cache_fit_tolerance: None,
                    record_bounds: support_parameter_bounds,
                });
            derived_surface_id
        } else {
            surface_id
        };
        candidate.model_mut().faces.push(Face {
            id: face_id.clone(),
            shell: shell_id.clone(),
            surface: face_surface_id,
            sense: Sense::Forward,
            loops: loop_ids,
            name: None,
            color: None,
            tolerance: (face_tolerance > 0.0).then_some(face_tolerance),
        });
        candidate.model_mut().shells.push(Shell {
            id: shell_id.clone(),
            region: region_id.clone(),
            faces: vec![face_id],
            wire_edges: Vec::new(),
            free_vertices: Vec::new(),
        });
        candidate.model_mut().regions.push(Region {
            id: region_id.clone(),
            body: body_id.clone(),
            shells: vec![shell_id],
        });
        candidate.model_mut().bodies.push(Body {
            id: body_id,
            kind: BodyKind::Sheet,
            regions: vec![region_id],
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        candidate.model_mut().finalize();
        staged.push((
            entry.sequence,
            candidate,
            candidate_boundary_vertex_derivations,
        ));
    }
    drop(carrier_index);
    let mut commit_session = CommitSession::new(ir);
    for (sequence, candidate, derivations) in staged {
        if commit_session.commit_model(candidate, ir).is_err() {
            let entry = entries
                .get(&sequence)
                .copied()
                .expect("staged trimming entry came from the directory");
            losses.push(entity_loss(
                entry,
                "trimmed sheet candidate failed neutral validation",
            ));
            continue;
        }
        decoded.insert(sequence);
        boundary_vertex_derivations.extend(derivations);
    }

    (
        ProjectionOutcome { decoded, losses },
        boundary_vertex_derivations,
    )
}

#[cfg(test)]
mod tests;
