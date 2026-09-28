// SPDX-License-Identifier: Apache-2.0
//! Face-local trimmed-surface projection.

use super::composite::{bounded_nurbs_for_curve_with_tolerance, CompositeIndex};
use super::geometry::{
    linear_nurbs_parameters, planar_polyline_has_self_intersection,
    planar_polylines_intersect, plane_coordinates, source_object, BoundaryEndpoint,
    BoundaryVertexDerivation, BoundaryVertexSourceEndpoint, DeclaredInterval, ProjectionOutcome,
};
use super::{affine_parameter_map, line_directrix, pointer};
use crate::decode_resource::{copy_optional_identity, format_retained, insert_optional_btree_set, reserve_optional_vec, reserve_vec, reserve_vec_growth};
use crate::directory::{DirectoryEntry, UseFlag};
use crate::global::{ProjectedGlobal, RealPrecision};
use crate::loss::IgesLossCode;
use crate::parameter::{ParameterRecord, TokenValue};
use cadmpeg_core::decode::{u64_from_index, DecodeContext};
use cadmpeg_core::CodecError;
use cadmpeg_ir::draft::{CommitSession, ModelDraft};
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::pcurve::PcurveMetadata;
use cadmpeg_ir::geometry::{
    nurbs::{NurbsCurve, NurbsError, NurbsPoles3},
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs, PcurveNurbsPoles, WeightedPole2},
    ProceduralSurface, ProceduralSurfaceDefinition, RecordBounds, SolvedCurveGeometry,
    SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use cadmpeg_ir::ids::{CurveId, ProceduralSurfaceId, SurfaceId, VertexId};
use cadmpeg_ir::index::ModelIndex;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::topology::{
    Body, BodyKind, Coedge, Edge, Face, Loop, PcurveUse, Point, Region, Sense, Shell, Vertex,
};
use cadmpeg_ir::units::FinitePoint2;
use cadmpeg_ir::CadIr;
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

struct BoundarySegment {
    model_curve: u32,
    pcurves: Vec<u32>,
    sense: Sense,
    parameter_curves_authoritative: bool,
}

struct BoundaryDefinition {
    surface: u32,
    segments: Vec<BoundarySegment>,
}

struct BoundaryItem<'a> {
    segment: &'a BoundarySegment,
    model_curve: CurveId,
    source_edge: Edge,
    start: FinitePoint3,
    end: FinitePoint3,
    pcurves: Vec<(PcurveGeometry, [f64; 2])>,
}

#[derive(Debug)]
enum BoundaryEdgeSelectionError {
    MissingEndpoints,
    InvalidRange,
    Ambiguous,
    PcurveDisagreement,
    Resource(CodecError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundaryVertexClusterError {
    NonTransitive,
}

#[derive(Debug)]
enum BoundaryVertexCreationError {
    Cluster(BoundaryVertexClusterError),
    Resource(CodecError),
}

impl From<BoundaryVertexClusterError> for BoundaryVertexCreationError {
    fn from(error: BoundaryVertexClusterError) -> Self {
        Self::Cluster(error)
    }
}

impl From<CodecError> for BoundaryVertexCreationError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

#[derive(Debug, PartialEq)]
struct BoundaryVertexCluster {
    representative: FinitePoint3,
    members: Vec<usize>,
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
    positions: &[FinitePoint3],
    tolerance: cadmpeg_ir::scalar::PositiveReal,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<BoundaryVertexCluster>, BoundaryVertexCreationError> {
    let tolerance = tolerance.get();
    let count = u64_from_index(positions.len());
    let pair_count = if count == 0 { 0 } else { count.checked_mul(count - 1).ok_or_else(|| cadmpeg_core::decode::refuse_local_limit("iges boundary clustering comparisons", u64::MAX, 1))? / 2 };
    ctx.charge_work(pair_count, "iges boundary clustering comparisons")?;
    let mut parents = reserve_vec(ctx, positions.len(), "iges boundary cluster parents")?;
    parents.extend(0..positions.len());
    for (left_index, left) in positions.iter().enumerate() {
        for (right_index, right) in positions.iter().enumerate().skip(left_index + 1) {
            if !close(left.get(), right.get(), tolerance) {
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
        if !members_by_root.contains_key(&root) {
            ctx.charge_collection_items(1, "iges boundary cluster roots")?;
        }
        let members = members_by_root.entry(root).or_default();
        reserve_vec_growth(ctx, members, 1, "iges boundary cluster members")?;
        members.push(index);
    }
    let mut clusters = reserve_vec(ctx, members_by_root.len(), "iges boundary cluster slots")?;
    ctx.charge_work(pair_count, "iges boundary cluster transitivity comparisons")?;
    for members in members_by_root.into_values() {
        if members.iter().enumerate().any(|(offset, left)| {
            members
                .iter()
                .skip(offset + 1)
                .any(|right| !close(positions[*left].get(), positions[*right].get(), tolerance))
        }) {
            return Err(BoundaryVertexClusterError::NonTransitive.into());
        }
        let representative = members
            .iter()
            .copied()
            .min_by(|left, right| {
                point_order(positions[*left].get(), positions[*right].get())
                    .then_with(|| left.cmp(right))
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
    stem: &crate::ids::Stem,
    source_entity: &str,
    boundary: usize,
    source_endpoints: &[BoundaryVertexSourceEndpoint],
    tolerance: cadmpeg_ir::scalar::PositiveReal,
    sequences: &mut super::geometry::SourceSequences,
    ctx: &DecodeContext<'_>,
) -> Result<(Vec<VertexId>, Vec<BoundaryVertexDerivation>), BoundaryVertexCreationError> {
    let mut positions = reserve_vec(ctx, source_endpoints.len(), "iges boundary endpoint positions")?;
    positions.extend(source_endpoints.iter().map(|endpoint| endpoint.position));
    let clusters = cluster_boundary_positions(&positions, tolerance, ctx)?;
    let mut vertex_ids = reserve_vec(ctx, positions.len(), "iges boundary endpoint vertex slots")?;
    vertex_ids.resize(positions.len(), None);
    let mut derivations = reserve_vec(ctx, clusters.len(), "iges boundary vertex derivations")?;
    for (index, cluster) in clusters.into_iter().enumerate() {
        let point_id = crate::ids::point(&stem.slot(boundary).slot(index));
        reserve_vec_growth(ctx, &mut candidate.model_mut().points, 1, "iges boundary points")?;
        reserve_vec_growth(ctx, &mut candidate.model_mut().vertices, 1, "iges boundary vertices")?;
        sequences.record_point(&point_id, stem, Some(ctx))?;
        let vertex_id = crate::ids::vertex(&stem.slot(boundary).slot(index));
        crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
        candidate.model_mut().points.push(Point::new(
            crate::decode_resource::clone_optional_identity(Some(ctx), &point_id, "iges trimming identity copy")?,
            cluster.representative,
            None,
        ));
        crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
        candidate.model_mut().vertices.push(Vertex {
            id: crate::decode_resource::clone_optional_identity(Some(ctx), &vertex_id, "iges trimming identity copy")?,
            point: point_id,
            tolerance: Some(tolerance),
        });
        let mut derivation_endpoints = reserve_vec(ctx, cluster.members.len(), "iges boundary derivation endpoints")?;
        for member in &cluster.members {
            let endpoint = &source_endpoints[*member];
            derivation_endpoints.push(BoundaryVertexSourceEndpoint {
                edge: format_retained(ctx, format_args!("{}", endpoint.edge), "iges boundary derivation edge text")?,
                endpoint: endpoint.endpoint,
                position: endpoint.position,
            });
        }
        derivations.push(BoundaryVertexDerivation {
            source_entity: format_retained(ctx, format_args!("{source_entity}"), "iges boundary derivation source text")?,
            vertex: crate::decode_resource::clone_optional_identity(Some(ctx), &vertex_id, "iges trimming identity copy")?,
            representative: cluster.representative,
            tolerance: tolerance.get(),
            source_endpoints: derivation_endpoints,
        });
        for member in cluster.members {
            vertex_ids[member] = Some(crate::decode_resource::clone_optional_identity(Some(ctx), &vertex_id, "iges trimming identity copy")?);
        }
    }
    let mut result_ids = reserve_vec(ctx, vertex_ids.len(), "iges boundary result vertex ids")?;
    result_ids.extend(vertex_ids.into_iter().flatten());
    Ok((result_ids, derivations))
}

fn point_position(index: &ModelIndex<'_>, id: &VertexId) -> Option<FinitePoint3> {
    let point_id = &index.vertices(id.as_str())?.point;
    index.points(point_id.as_str()).map(Point::position)
}

pub(super) struct PcurveSupport<'a> {
    pub(super) surface_id: &'a SurfaceId,
    pub(super) geometry: &'a SurfaceGeometry,
    pub(super) factor: f64,
}

#[derive(Clone, Copy)]
enum ProceduralSourceParameterMap {
    NotApplicable,
    Unavailable,
    Mapped((f64, f64, f64, f64)),
}

fn procedural_source_parameter_map(
    ir: &CadIr,
    support: &PcurveSupport<'_>,
) -> ProceduralSourceParameterMap {
    let procedural = ir.model.procedural_surfaces.iter().find(|procedural| {
        ir.model.procedural_surface_owner(&procedural.id) == Some(support.surface_id)
    });
    if let Some(procedural) = procedural.filter(|procedural| {
        matches!(
            procedural.definition(),
            ProceduralSurfaceDefinition::Extrusion(_) | ProceduralSurfaceDefinition::Revolution(_)
        )
    }) {
        return procedural_pcurve_parameter_map(ir, &procedural.id).map_or(
            ProceduralSourceParameterMap::Unavailable,
            ProceduralSourceParameterMap::Mapped,
        );
    }
    match support.geometry {
        SurfaceGeometry::Procedural { construction, .. } => {
            procedural_pcurve_parameter_map(ir, construction).map_or(
                ProceduralSourceParameterMap::Unavailable,
                ProceduralSourceParameterMap::Mapped,
            )
        }
        SurfaceGeometry::Solved(_) => ProceduralSourceParameterMap::NotApplicable,
    }
}

fn pcurve_parameter_map(ir: &CadIr, support: &PcurveSupport<'_>) -> Option<(f64, f64, f64, f64)> {
    match procedural_source_parameter_map(ir, support) {
        ProceduralSourceParameterMap::Mapped(parameter_map) => {
            source_parameter_map_to_neutral(parameter_map, support.factor)
        }
        ProceduralSourceParameterMap::Unavailable => None,
        ProceduralSourceParameterMap::NotApplicable => match support.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_)) => Some((1.0, 0.0, 1.0, 0.0)),
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(_)) => {
                Some((1.0 / support.factor, 0.0, 1.0, 0.0))
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(_)) => {
                Some((1.0 / support.factor, 0.0, 1.0, 0.0))
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(_)) => {
                Some((1.0 / support.factor, 0.0, 1.0 / support.factor, 0.0))
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_)) => {
                Some((1.0 / support.factor, 0.0, 1.0 / support.factor, 0.0))
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_)) => {
                Some((1.0 / support.factor, 0.0, 1.0 / support.factor, 0.0))
            }
            SurfaceGeometry::Procedural { .. } => None,
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Polygonal(_)
                | SolvedSurfaceGeometry::Transformed(_)
                | SolvedSurfaceGeometry::Unknown { .. },
            ) => None,
        },
    }
}

fn source_parameter_map_to_neutral(
    (u_factor, u_offset, v_factor, v_offset): (f64, f64, f64, f64),
    length_factor_mm: f64,
) -> Option<(f64, f64, f64, f64)> {
    // A generic projected curve is length-scaled, but a procedural pcurve
    // stores source surface parameters. Remove that conversion before the
    // procedural map evaluates those parameters on the neutral surface.
    if !length_factor_mm.is_finite() || length_factor_mm <= 0.0 {
        return None;
    }
    let map = (
        u_factor / length_factor_mm,
        u_offset,
        v_factor / length_factor_mm,
        v_offset,
    );
    (map.0.is_finite() && map.1.is_finite() && map.2.is_finite() && map.3.is_finite())
        .then_some(map)
}

fn source_parameter_point_to_neutral(
    point: Point2,
    (u_factor, u_offset, v_factor, v_offset): (f64, f64, f64, f64),
    length_factor_mm: f64,
) -> Point2 {
    let source_u = point.u / length_factor_mm;
    let source_v = point.v / length_factor_mm;
    Point2::new(
        source_u.mul_add(u_factor, u_offset),
        source_v.mul_add(v_factor, v_offset),
    )
}

pub(super) fn pcurve_geometry(
    ir: &CadIr,
    sequence: u32,
    support: &PcurveSupport<'_>,
    tolerance: Option<f64>,
    ctx: Option<&DecodeContext<'_>>,
    composite_index: Option<&CompositeIndex>,
) -> Result<Option<(PcurveGeometry, [f64; 2])>, super::composite::CompositeCurveError> {
    let curve_id = crate::ids::curve(&crate::ids::Stem::directory(sequence));
    let Some((nurbs, range)) =
        bounded_nurbs_for_curve_with_tolerance(ir, &curve_id, tolerance, ctx, composite_index)?
    else {
        return Ok(None);
    };
    let source_parameter_map = match procedural_source_parameter_map(ir, support) {
        ProceduralSourceParameterMap::Mapped(parameter_map) => Some(parameter_map),
        ProceduralSourceParameterMap::NotApplicable | ProceduralSourceParameterMap::Unavailable => {
            None
        }
    };
    let Some((u_factor, u_offset, v_factor, v_offset)) = pcurve_parameter_map(ir, support) else {
        return Ok(None);
    };
    let map_point = |point: FinitePoint3| -> Result<FinitePoint2, NurbsError> {
        let point = point.get();
        let mapped = source_parameter_map.map_or_else(
            || Point2::new(point.x.mul_add(u_factor, u_offset), point.y.mul_add(v_factor, v_offset)),
            |(u_factor, u_offset, v_factor, v_offset)| {
                source_parameter_point_to_neutral(
                    Point2::new(point.x, point.y),
                    (u_factor, u_offset, v_factor, v_offset),
                    support.factor,
                )
            },
        );
        FinitePoint2::new(mapped)
            .ok_or_else(|| NurbsError::Structure("control_points contains a non-finite point".into()))
    };
    let (degree, knots, poles, periodic) = nurbs.into_parts();
    let poles = match poles {
        NurbsPoles3::Polynomial { points } => {
            let mut mapped = reserve_optional_vec(ctx, points.len(), "iges pcurve mapped polynomial poles")?;
            for point in points {
                mapped.push(map_point(point)?);
            }
            PcurveNurbsPoles::Polynomial { points: mapped }
        }
        NurbsPoles3::Rational { points } => {
            let mut mapped = reserve_optional_vec(ctx, points.len(), "iges pcurve mapped rational poles")?;
            for pole in points {
                mapped.push(WeightedPole2 {
                    point: map_point(pole.point)?,
                    weight: pole.weight,
                });
            }
            PcurveNurbsPoles::Rational { points: mapped }
        }
    };
    let parameter_curve = PcurveNurbs::from_admitted_parts(degree, knots, poles, periodic)?;
    Ok(Some((
        PcurveGeometry::Nurbs {
            nurbs: parameter_curve,
        },
        range,
    )))
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
    let Some([Some(carrier_start), Some(carrier_end), _, _]) =
        procedural.record_bounds().map(RecordBounds::get)
    else {
        return None;
    };
    let carrier_interval = [carrier_start, carrier_end];
    if carrier_interval[0] >= carrier_interval[1] {
        return None;
    }
    let mut u_map = (1.0, 0.0);
    let mut v_map = (1.0, 0.0);
    match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
            let directrix = definition_payload.directrix();
            let parameter_interval = definition_payload.parameter_interval();
            {
                if line_directrix(ir, directrix) {
                    u_map = affine_parameter_map([0.0, 1.0], carrier_interval)?;
                } else if let Some(parameter_interval) = parameter_interval {
                    u_map = affine_parameter_map(parameter_interval.get(), carrier_interval)?;
                }
            }
        }
        ProceduralSurfaceDefinition::Revolution(definition_payload) => {
            let directrix = definition_payload.directrix();
            let angular_interval = definition_payload.angular_interval().endpoints();
            let angular_parameter_interval = definition_payload
                .angular_parameter_interval()
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints);
            let parameter_interval = definition_payload
                .parameter_interval()
                .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints);
            let transposed = definition_payload.transposed();
            {
                let directrix_map = if line_directrix(ir, directrix) {
                    affine_parameter_map([0.0, 1.0], carrier_interval)?
                } else if let Some(parameter_interval) = parameter_interval {
                    affine_parameter_map(parameter_interval, carrier_interval)?
                } else {
                    (1.0, 0.0)
                };
                let angular_map = match angular_parameter_interval {
                    Some(parameter_interval) => {
                        affine_parameter_map(parameter_interval, angular_interval)?
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
        }
        _ => return None,
    }
    Some((u_map.0, u_map.1, v_map.0, v_map.1))
}

fn native_sequence_from_id(id: &str, prefix: &str) -> Option<u32> {
    let suffix = id.strip_prefix(prefix)?;
    let end = suffix
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(suffix.len());
    (end > 0).then(|| suffix[..end].parse().ok())?
}

fn parameter_curve_carrier_id(
    sequence: u32,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
) -> Option<CurveId> {
    let entry = entries.get(&sequence).copied()?;
    let carrier_sequence = if entry.entity_type == 142 && entry.form == 0 {
        records
            .get(&sequence)
            .and_then(|record| record.integer(3))
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1)?
    } else {
        sequence
    };
    Some(crate::ids::curve(&crate::ids::Stem::directory(
        carrier_sequence,
    )))
}

fn surface_parameter_bound_intervals(
    bounds: Option<[Option<f64>; 4]>,
    surface_id: &SurfaceId,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    precision: RealPrecision,
) -> Option<[Option<DeclaredInterval>; 4]> {
    let bounds = bounds?;
    let mut intervals = bounds.map(|bound| bound.map(|value| DeclaredInterval::around(value, 0.0)));
    let Some(sequence) = native_sequence_from_id(surface_id.as_str(), "iges:model:surface#D")
    else {
        return Some(intervals);
    };
    let Some(entry) = entries.get(&sequence).copied() else {
        return Some(intervals);
    };
    if entry.entity_type != 128 {
        return Some(intervals);
    }
    let Some(record) = records.get(&sequence).copied() else {
        return Some(intervals);
    };
    if let Some(declared) = super::surfaces::type128_parameter_bound_intervals(record, precision) {
        for (bound, declared) in intervals.iter_mut().zip(declared) {
            if bound.is_some() {
                *bound = Some(declared);
            }
        }
    }
    Some(intervals)
}

fn source_curve_control_intervals(
    ir: &CadIr,
    curve_id: &CurveId,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    precision: RealPrecision,
    factor: f64,
    active: &mut BTreeSet<CurveId>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<[DeclaredInterval; 3]>>, CodecError> {
    let _nested = ctx.enter_nested("iges source curve intervals")?;
    if active.contains(curve_id) {
        return Ok(None);
    }
    let active_id = copy_optional_identity(Some(ctx), curve_id.as_str(), "iges source active curve ID")?;
    insert_optional_btree_set(Some(ctx), active, active_id, "iges source active curve nodes")?;
    let result = (|| -> Result<Option<Vec<[DeclaredInterval; 3]>>, CodecError> {
        let Some(curve) = ir.model.curves.iter().find(|curve| curve.id == *curve_id) else {
            return Ok(None);
        };
        if let Some(sequence) = native_sequence_from_id(curve_id.as_str(), "iges:model:curve#D") {
            let Some(entry) = entries.get(&sequence).copied() else {
                return Ok(None);
            };
            if entry.entity_type == 102 && entry.form == 0 {
                let Some(record) = records.get(&sequence).copied() else {
                    return Ok(None);
                };
                let Some(child_count) = record.count(1) else {
                    return Ok(None);
                };
                let mut child_ids = reserve_vec(ctx, child_count, "iges source composite child IDs")?;
                for offset in 0..child_count {
                    let Some(child_id) = (|| {
                        let child_sequence = record
                            .integer(offset.checked_add(2)?)
                            .and_then(|value| u32::try_from(value).ok())?;
                        parameter_curve_carrier_id(child_sequence, entries, records)
                    })() else {
                        return Ok(None);
                    };
                    child_ids.push(child_id);
                }
                let mut controls = Vec::new();
                for child_id in child_ids {
                    let Some(child) = source_curve_control_intervals(
                        ir, &child_id, entries, records, precision, factor, active, ctx,
                    )? else {
                        return Ok(None);
                    };
                    reserve_vec_growth(ctx, &mut controls, child.len(), "iges source composite controls")?;
                    controls.extend(child);
                }
                return Ok((!controls.is_empty()).then_some(controls));
            }
        }
        match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Composite { segments, .. }) => {
                let mut controls = Vec::new();
                for segment in segments {
                    let Some(child) = source_curve_control_intervals(
                        ir,
                        &segment.curve,
                        entries,
                        records,
                        precision,
                        factor,
                        active,
                        ctx,
                    )? else {
                        return Ok(None);
                    };
                    reserve_vec_growth(ctx, &mut controls, child.len(), "iges source solved composite controls")?;
                    controls.extend(child);
                }
                Ok((!controls.is_empty()).then_some(controls))
            }
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                if (0..nurbs.pole_count())
                    .any(|index| nurbs.pole_rows().weight_at(index).is_some_and(|weight| weight <= 0.0))
                {
                    return Ok(None);
                }
                let exact = || -> Result<Vec<[DeclaredInterval; 3]>, CodecError> {
                    let mut controls = reserve_vec(ctx, nurbs.pole_count(), "iges source exact controls")?;
                    for index in 0..nurbs.pole_count() {
                        let point = nurbs.pole_rows().point_at(index).ok_or_else(|| CodecError::malformed("source curve pole is missing"))?.get();
                        controls.push([point.x, point.y, point.z].map(|value| DeclaredInterval::around(value, 0.0)));
                    }
                    Ok(controls)
                };
                let Some(sequence) =
                    native_sequence_from_id(curve_id.as_str(), "iges:model:curve#D")
                else {
                    return Ok(Some(exact()?));
                };
                let Some(entry) = entries.get(&sequence).copied() else {
                    return Ok(Some(exact()?));
                };
                if entry.entity_type != 126 {
                    return Ok(Some(exact()?));
                }
                if entry.transform != 0 {
                    return Ok(None);
                }
                let Some(record) = records.get(&sequence).copied() else {
                    return Ok(None);
                };
                let Some(mut raw_controls) = super::geometry::type126_declared_control_points(record, precision, ctx)? else {
                    return Ok(None);
                };
                if raw_controls.len() != nurbs.pole_count() {
                    return Ok(None);
                }
                for control in &mut raw_controls {
                    *control = control.map(|value| value.scale(factor));
                }
                Ok(Some(raw_controls))
            }
            _ => Ok(None),
        }
    })();
    active.remove(curve_id);
    result
}

fn affine_parameter_interval(
    value: DeclaredInterval,
    factor: f64,
    offset: f64,
) -> Option<DeclaredInterval> {
    let mapped = if factor == 1.0 && offset == 0.0 {
        value
    } else {
        value
            .scale(factor)
            .add(DeclaredInterval::around(offset, 0.0))
    };
    mapped.is_finite().then_some(mapped)
}

fn parameter_interval_reaches_bounds(
    value: DeclaredInterval,
    lower: Option<DeclaredInterval>,
    upper: Option<DeclaredInterval>,
) -> bool {
    // A source real that reaches a support bound can represent the boundary
    // value. This is representation uncertainty, not a receiver tolerance;
    // an interval separated from either finite bound remains invalid.
    value.is_finite()
        && lower.is_none_or(|bound| bound.is_finite() && value.upper_bound() >= bound.lower_bound())
        && upper.is_none_or(|bound| bound.is_finite() && value.lower_bound() <= bound.upper_bound())
}

fn source_curve_control_polygon_within_bounds(
    ir: &CadIr,
    curve_id: &CurveId,
    support: &PcurveSupport<'_>,
    bounds: Option<[Option<DeclaredInterval>; 4]>,
    entries: &BTreeMap<u32, &DirectoryEntry>,
    records: &BTreeMap<u32, &ParameterRecord>,
    precision: RealPrecision,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(bounds) = bounds else {
        return Ok(true);
    };
    let Some((u_factor, u_offset, v_factor, v_offset)) = pcurve_parameter_map(ir, support) else {
        return Ok(false);
    };
    let Some(controls) = source_curve_control_intervals(
        ir,
        curve_id,
        entries,
        records,
        precision,
        support.factor,
        &mut BTreeSet::new(),
        ctx,
    )? else {
        return Ok(false);
    };
    Ok(!controls.is_empty()
        && controls.into_iter().all(|[u, v, _]| {
            let Some(u) = affine_parameter_interval(u, u_factor, u_offset) else {
                return false;
            };
            let Some(v) = affine_parameter_interval(v, v_factor, v_offset) else {
                return false;
            };
            parameter_interval_reaches_bounds(u, bounds[0], bounds[1])
                && parameter_interval_reaches_bounds(v, bounds[2], bounds[3])
        }))
}

fn linear_model_nurbs_points(
    nurbs: &NurbsCurve,
    range: [f64; 2],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    if (0..nurbs.pole_count())
        .any(|index| nurbs.pole_rows().weight_at(index).is_some_and(|weight| weight != 1.0)) {
        return Ok(None);
    }
    let Some(parameters) = linear_nurbs_parameters(
        nurbs.degree(),
        nurbs.knots(),
        nurbs.pole_count(),
        nurbs.periodic(),
        range,
    ) else {
        return Ok(None);
    };
    let mut points = reserve_vec(
        ctx,
        parameters.clone().count(),
        "iges linear model boundary points",
    )?;
    for parameter in parameters {
        let Some(point) =
            finite_or_refusal(cadmpeg_ir::eval::nurbs_curve_point_at(nurbs, parameter))?
        else {
            return Ok(None);
        };
        points.push(point.get());
    }
    Ok(Some(points))
}

fn linear_pcurve_points(
    geometry: &PcurveGeometry,
    range: [f64; 2],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<[f64; 2]>>, CodecError> {
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        return Ok(None);
    };
    if (0..nurbs.pole_rows().count())
        .any(|index| nurbs.pole_rows().weight_at(index).is_some_and(|weight| weight != 1.0)) {
        return Ok(None);
    }
    let Some(parameters) = linear_nurbs_parameters(
        nurbs.degree(),
        nurbs.knots(),
        nurbs.pole_rows().count(),
        nurbs.periodic(),
        range,
    ) else {
        return Ok(None);
    };
    let mut points = reserve_vec(
        ctx,
        parameters.clone().count(),
        "iges linear parameter boundary points",
    )?;
    for parameter in parameters {
        let Some(point) = finite_or_refusal(cadmpeg_ir::eval::pcurve_uv(geometry, parameter))?
        else {
            return Ok(None);
        };
        points.push([point.u, point.v]);
    }
    Ok(Some(points))
}

fn append_path<T: Copy + PartialEq>(
    target: &mut Vec<T>,
    path: Vec<T>,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(first) = path.first().copied() else {
        return Ok(false);
    };
    if target.last().is_some_and(|last| *last != first) {
        return Ok(false);
    }
    let additional = if target.is_empty() {
        path.len()
    } else {
        path.len() - 1
    };
    reserve_vec_growth(ctx, target, additional, "iges linear boundary path")?;
    if target.is_empty() {
        target.extend(path);
    } else {
        target.extend(path.into_iter().skip(1));
    }
    Ok(true)
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    let mut points = Vec::new();
    for item in items {
        let Some(curve) = index.curves(item.model_curve.as_str()) else {
            return Ok(None);
        };
        let mut curve_points = match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Line(_)) => {
                let mut line = reserve_vec(ctx, 2, "iges linear model boundary line")?;
                line.push(item.start.get());
                line.push(item.end.get());
                line
            }
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                let Some(range) = item.source_edge.param_range() else {
                    return Ok(None);
                };
                let Some(points) = linear_model_nurbs_points(nurbs, range.get(), ctx)? else {
                    return Ok(None);
                };
                points
            }
            _ => return Ok(None),
        };
        if curve_points.first().copied() != Some(item.start.get())
            || curve_points.last().copied() != Some(item.end.get())
        {
            return Ok(None);
        }
        if item.segment.sense == Sense::Reversed {
            curve_points.reverse();
        }
        if !append_path(&mut points, curve_points, ctx)? {
            return Ok(None);
        }
    }
    normalize_model_ring_endpoints(&mut points, closure_tolerance);
    Ok(Some(points))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundarySpace {
    Parameter,
    Model,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundarySurfaceKind {
    Bounded,
    Trimmed,
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
    surface_kind: BoundarySurfaceKind,
    ctx: &DecodeContext<'_>,
) -> Result<Option<LinearBoundaryGeometry>, CodecError> {
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) = support else {
        return Ok(None);
    };
    let origin = plane_surface.origin().get();
    let normal = plane_surface.frame().axis().as_raw();
    let Some(model_points) = linear_boundary_model_points(items, index, closure_tolerance, ctx)?
    else {
        return Ok(None);
    };
    let model_plane = (origin, *normal);
    for item in items {
        let Some(curve) = index.curves(item.model_curve.as_str()) else {
            return Ok(None);
        };
        let Some(geometry) = curve.geometry.solved() else {
            return Ok(None);
        };
        if !super::geometry::curve_geometry_coplanar(
            geometry,
            index,
            cadmpeg_ir::transform::Transform::identity(),
            model_plane,
            resolution,
            &mut BTreeSet::new(),
            Some(ctx),
        )? {
            return Ok(None);
        }
    }
    let Some(model_coordinates) = plane_coordinates(&model_points, model_plane, ctx)? else {
        return Ok(None);
    };
    if surface_kind == BoundarySurfaceKind::Trimmed
        && items
            .iter()
            .any(|item| item.segment.parameter_curves_authoritative)
    {
        if !items
            .iter()
            .all(|item| item.segment.parameter_curves_authoritative)
        {
            return Ok(None);
        }
        let mut parameter_points = Vec::new();
        for item in items {
            if item.pcurves.is_empty() {
                return Ok(None);
            }
            for (geometry, range) in &item.pcurves {
                let Some(points) = linear_pcurve_points(geometry, *range, ctx)? else {
                    return Ok(None);
                };
                if !append_path(&mut parameter_points, points, ctx)? {
                    return Ok(None);
                }
            }
        }
        normalize_parameter_ring_endpoints(&mut parameter_points, closure_tolerance);
        Ok(Some(LinearBoundaryGeometry::Parameter(parameter_points)))
    } else {
        Ok(Some(LinearBoundaryGeometry::Model(model_coordinates)))
    }
}

#[derive(Debug)]
struct SimpleRing(Vec<[f64; 2]>);

#[derive(Debug)]
struct NonSimpleRing;

impl SimpleRing {
    fn new(points: Vec<[f64; 2]>) -> Result<Self, NonSimpleRing> {
        if points.len() < 4
            || points.first() != points.last()
            || points
                .iter()
                .flatten()
                .any(|coordinate| !coordinate.is_finite())
        {
            return Err(NonSimpleRing);
        }
        if points.windows(2).any(|segment| segment[0] == segment[1]) {
            return Err(NonSimpleRing);
        }
        let last = points.len() - 1;
        for first in 0..last {
            for second in first + 1..last {
                if points[first] == points[second] {
                    return Err(NonSimpleRing);
                }
            }
        }
        if planar_polyline_has_self_intersection(&points) {
            return Err(NonSimpleRing);
        }
        Ok(Self(points))
    }

    fn first(&self) -> [f64; 2] {
        self.0[0]
    }

    fn interior(&self) -> &[[f64; 2]] {
        &self.0[..self.0.len() - 1]
    }

    fn points(&self) -> &[[f64; 2]] {
        &self.0
    }
}

fn planar_point_is_strictly_inside(point: [f64; 2], ring: &SimpleRing) -> bool {
    if ring.points().windows(2).any(|segment| {
        super::geometry::planar_segments_contain_point(point, [segment[0], segment[1]])
    }) {
        return false;
    }
    let mut inside = false;
    for segment in ring.points().windows(2) {
        let [left, right] = [segment[0], segment[1]];
        if (left[1] > point[1]) != (right[1] > point[1]) {
            let crossing =
                left[0] + (right[0] - left[0]) * (point[1] - left[1]) / (right[1] - left[1]);
            if point[0] < crossing {
                inside = !inside;
            }
        }
    }
    inside
}

fn linear_boundary_rings(
    candidates: &[Option<LinearBoundaryGeometry>],
    space: BoundarySpace,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Result<Vec<SimpleRing>, NonSimpleRing>>, CodecError> {
    if candidates.is_empty() {
        return Ok(None);
    }
    let mut rings = reserve_vec(ctx, candidates.len(), "iges linear boundary ring slots")?;
    for candidate in candidates {
        let points = match (space, candidate.as_ref()) {
            (BoundarySpace::Parameter, Some(LinearBoundaryGeometry::Parameter(points)))
            | (BoundarySpace::Model, Some(LinearBoundaryGeometry::Model(points))) => {
                points
            }
            _ => return Ok(None),
        };
        let mut copied = reserve_vec(ctx, points.len(), "iges linear boundary ring points")?;
        copied.extend_from_slice(points);
        match SimpleRing::new(copied) {
            Ok(ring) => rings.push(ring),
            Err(error) => return Ok(Some(Err(error))),
        }
    }
    Ok(Some(Ok(rings)))
}

fn inner_boundaries_are_disjoint_and_inside(outer: &SimpleRing, inners: &[SimpleRing]) -> bool {
    for inner in inners {
        if planar_polylines_intersect(outer.points(), inner.points())
            || inner
                .interior()
                .iter()
                .any(|point| !planar_point_is_strictly_inside(*point, outer))
        {
            return false;
        }
    }
    inners.iter().enumerate().all(|(left_index, left)| {
        inners.iter().skip(left_index + 1).all(|right| {
            !planar_polylines_intersect(left.points(), right.points())
                && !planar_point_is_strictly_inside(left.first(), right)
                && !planar_point_is_strictly_inside(right.first(), left)
        })
    })
}

fn linear_boundary_relationship_is_valid(
    rings: Result<&[SimpleRing], &NonSimpleRing>,
    surface_kind: BoundarySurfaceKind,
    has_explicit_outer: bool,
    support: &SurfaceGeometry,
    support_bounds: Option<[Option<f64>; 4]>,
    periodic_parameters: [bool; 2],
) -> Option<bool> {
    let rings = match rings {
        Ok(rings) => rings,
        Err(NonSimpleRing) => return Some(false),
    };
    if surface_kind == BoundarySurfaceKind::Bounded {
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
                ring.interior().iter().any(|point| {
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
        None if !matches!(
            support,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) =>
        {
            return None
        }
        None => {}
    }
    Some(rings.iter().enumerate().all(|(left_index, left)| {
        rings.iter().skip(left_index + 1).all(|right| {
            !planar_polylines_intersect(left.points(), right.points())
                && !planar_point_is_strictly_inside(left.first(), right)
                && !planar_point_is_strictly_inside(right.first(), left)
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
    ctx: &DecodeContext<'_>,
) -> Result<Option<()>, CodecError> {
    let count = controls.len();
    let Some(span) = knots
        .windows(2)
        .position(|pair| pair[0] <= knot && knot < pair[1]) else {
        return Ok(None);
    };
    let multiplicity = knots.iter().filter(|candidate| **candidate == knot).count();
    if multiplicity >= degree {
        return Ok(Some(()));
    }
    let Some((left_end, tail_start, inserted_count)) = span
        .checked_sub(degree)
        .zip(span.checked_sub(multiplicity))
        .zip(count.checked_add(1))
        .map(|((left_end, tail_start), count)| (left_end, tail_start, count)) else {
        return Ok(None);
    };
    let mut inserted = reserve_vec(ctx, inserted_count, "iges pcurve inserted controls")?;
    inserted.resize(inserted_count, [0.0; 4]);
    inserted[..=left_end].copy_from_slice(&controls[..=left_end]);
    inserted[tail_start + 1..].copy_from_slice(&controls[tail_start..]);
    for index in left_end + 1..=tail_start {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return Ok(None);
        }
        let alpha = (knot - knots[index]) / denominator;
        inserted[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    reserve_vec_growth(ctx, knots, 1, "iges pcurve inserted knots")?;
    knots.insert(span + 1, knot);
    *controls = inserted;
    Ok(Some(()))
}

fn homogeneous_pcurve_spans(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; 4]>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<HomogeneousPcurveSpan>>, CodecError> {
    let Some(expected_knots) = controls.len().checked_add(degree).and_then(|value| value.checked_add(1)) else {
        return Ok(None);
    };
    if degree == 0
        || degree >= controls.len()
        || knots.len() != expected_knots
        || knots.iter().any(|knot| !knot.is_finite())
        || knots.windows(2).any(|pair| pair[0] > pair[1])
    {
        return Ok(None);
    }
    let Some(domain) = knots.get(degree).copied().zip(knots.get(controls.len()).copied()) else {
        return Ok(None);
    };
    let domain = [domain.0, domain.1];
    if domain[0] >= domain[1] {
        return Ok(None);
    }
    let mut copied_knots = reserve_vec(ctx, knots.len(), "iges pcurve knot copy")?;
    copied_knots.extend_from_slice(knots);
    let Some(internal_slice) = copied_knots.get(degree + 1..controls.len()) else {
        return Ok(None);
    };
    let mut internal = reserve_vec(ctx, internal_slice.len(), "iges pcurve internal knots")?;
    internal.extend(internal_slice.iter().copied().filter(|knot| domain[0] < *knot && *knot < domain[1]));
    internal.sort_by(f64::total_cmp);
    internal.dedup();
    for knot in internal {
        while copied_knots.iter().filter(|candidate| **candidate == knot).count() < degree {
            if insert_homogeneous_pcurve_knot(degree, &mut copied_knots, &mut controls, knot, ctx)?.is_none() {
                return Ok(None);
            }
        }
    }
    let mut spans = reserve_vec(ctx, controls.len(), "iges pcurve span descriptors")?;
    for span in degree..controls.len() {
        let Some((start, end)) = copied_knots.get(span).copied().zip(copied_knots.get(span + 1).copied()) else {
            return Ok(None);
        };
        if start >= end {
            continue;
        }
        let Some(start_index) = span.checked_sub(degree) else {
            return Ok(None);
        };
        let Some(span_controls) = controls.get(start_index..=span) else {
            return Ok(None);
        };
        let mut copied_controls = reserve_vec(ctx, span_controls.len(), "iges pcurve span controls")?;
        copied_controls.extend_from_slice(span_controls);
        spans.push(HomogeneousPcurveSpan {
            domain: [start, end],
            controls: copied_controls,
        });
    }
    Ok((!spans.is_empty()).then_some(spans))
}

fn split_homogeneous_pcurve(
    controls: &[[f64; 4]],
    parameter: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<HomogeneousPcurveSplit>, CodecError> {
    if controls.is_empty() || !parameter.is_finite() || !(0.0..=1.0).contains(&parameter) {
        return Ok(None);
    }
    let mut levels = reserve_vec(ctx, controls.len(), "iges pcurve split levels")?;
    let mut first = reserve_vec(ctx, controls.len(), "iges pcurve split first controls")?;
    first.extend_from_slice(controls);
    levels.push(first);
    while levels.last().is_some_and(|level| level.len() > 1) {
        let Some(previous) = levels.last() else {
            return Ok(None);
        };
        let mut next = reserve_vec(ctx, previous.len() - 1, "iges pcurve split level controls")?;
        for pair in previous.windows(2) {
            next.push(std::array::from_fn(|axis| {
                (1.0 - parameter) * pair[0][axis] + parameter * pair[1][axis]
            }));
        }
        levels.push(next);
    }
    let mut left = reserve_vec(ctx, levels.len(), "iges pcurve split left controls")?;
    let mut right = reserve_vec(ctx, levels.len(), "iges pcurve split right controls")?;
    for level in &levels {
        let Some(point) = level.first().copied() else { return Ok(None); };
        left.push(point);
    }
    for level in levels.iter().rev() {
        let Some(point) = level.last().copied() else { return Ok(None); };
        right.push(point);
    }
    Ok(Some((left, right)))
}

fn restrict_homogeneous_pcurve(
    controls: &[[f64; 4]],
    start: f64,
    end: f64,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<[f64; 4]>>, CodecError> {
    let _nested = ctx.enter_nested("iges pcurve restricted span")?;
    if start > end {
        let Some(mut restricted) = restrict_homogeneous_pcurve(controls, end, start, ctx)? else {
            return Ok(None);
        };
        restricted.reverse();
        return Ok(Some(restricted));
    }
    if start == end {
        let Some(point) = split_homogeneous_pcurve(controls, start, ctx)?
            .and_then(|(left, _)| left.into_iter().last()) else {
            return Ok(None);
        };
        let mut result = reserve_vec(ctx, 1, "iges pcurve restricted point")?;
        result.push(point);
        return Ok(Some(result));
    }
    let Some((left, _)) = split_homogeneous_pcurve(controls, end, ctx)? else {
        return Ok(None);
    };
    if start == 0.0 {
        return Ok(Some(left));
    }
    let relative_start = start / end;
    Ok(split_homogeneous_pcurve(&left, relative_start, ctx)?.map(|(_, right)| right))
}

#[cfg(test)]
fn pcurve_within_declared_bounds(
    geometry: &PcurveGeometry,
    range: [f64; 2],
    bounds: Option<[Option<f64>; 4]>,
    periodic: [bool; 2],
) -> bool {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service()).unwrap();
    let bounds = bounds
        .map(|bounds| bounds.map(|bound| bound.map(|value| DeclaredInterval::around(value, 0.0))));
    pcurve_within_declared_intervals(geometry, range, bounds, periodic, &ctx).unwrap()
}

fn pcurve_within_declared_intervals(
    geometry: &PcurveGeometry,
    range: [f64; 2],
    bounds: Option<[Option<DeclaredInterval>; 4]>,
    periodic: [bool; 2],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let Some(bounds) = bounds else {
        return Ok(true);
    };
    let PcurveGeometry::Nurbs { nurbs } = geometry else {
        return Ok(false);
    };
    let Some(degree) = usize::try_from(nurbs.degree()).ok() else {
        return Ok(false);
    };
    if !range[0].is_finite() || !range[1].is_finite() || range[0] >= range[1] {
        return Ok(false);
    }
    let mut controls = reserve_vec(ctx, nurbs.pole_rows().count(), "iges pcurve homogeneous controls")?;
    for index in 0..nurbs.pole_rows().count() {
        let Some(point) = nurbs.pole_rows().point_at(index).map(|point| point.get()) else {
            return Ok(false);
        };
        let weight = nurbs.pole_rows().weight_at(index).unwrap_or(1.0);
        if weight <= 0.0 {
            return Ok(false);
        }
        controls.push([weight, weight * point.u, weight * point.v, 0.0]);
    }
    let Some(spans) = homogeneous_pcurve_spans(degree, nurbs.knots(), controls, ctx)? else {
        return Ok(false);
    };
    let Some(first_span) = spans.first() else {
        return Ok(false);
    };
    let Some(last_span) = spans.last() else {
        return Ok(false);
    };
    if range[0] < first_span.domain[0] || range[1] > last_span.domain[1] {
        return Ok(false);
    }
    let in_bound =
        |value: f64, lower: Option<DeclaredInterval>, upper: Option<DeclaredInterval>| {
            value.is_finite()
                && lower.is_none_or(|lower| lower.is_finite() && value >= lower.lower_bound())
                && upper.is_none_or(|upper| upper.is_finite() && value <= upper.upper_bound())
        };
    let expand_periodic = |lower: Option<DeclaredInterval>,
                           upper: Option<DeclaredInterval>,
                           periodic: bool| match (lower, upper, periodic)
    {
        (Some(lower), Some(upper), true) => {
            let lower = DeclaredInterval::around(lower.lower_bound(), 0.0);
            let upper = DeclaredInterval::around(upper.upper_bound(), 0.0);
            let period = upper.subtract(lower);
            (period.is_finite() && period.is_strictly_positive())
                .then_some((Some(lower.subtract(period)), Some(upper.add(period))))
        }
        _ => Some((lower, upper)),
    };
    let Some((u_lower, u_upper)) = expand_periodic(bounds[0], bounds[1], periodic[0]) else {
        return Ok(false);
    };
    let Some((v_lower, v_upper)) = expand_periodic(bounds[2], bounds[3], periodic[1]) else {
        return Ok(false);
    };
    let mut covered = false;
    for span in spans {
        let start = range[0].max(span.domain[0]);
        let end = range[1].min(span.domain[1]);
        if start >= end {
            continue;
        }
        covered = true;
        let Some(local_start) =
            cadmpeg_ir::math::parameter_fraction(start, span.domain[0], span.domain[1])
        else {
            return Ok(false);
        };
        let Some(local_end) =
            cadmpeg_ir::math::parameter_fraction(end, span.domain[0], span.domain[1])
        else {
            return Ok(false);
        };
        let Some(restricted) =
            restrict_homogeneous_pcurve(&span.controls, local_start.get(), local_end.get(), ctx)?
        else {
            return Ok(false);
        };
        if restricted.iter().any(|control| {
            let weight = control[0];
            !weight.is_finite()
                || weight <= 0.0
                || !in_bound(control[1] / weight, u_lower, u_upper)
                || !in_bound(control[2] / weight, v_lower, v_upper)
        }) {
            return Ok(false);
        }
    }
    Ok(covered)
}

fn periodic_surface_parameters(surface: &SurfaceGeometry) -> [bool; 2] {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface)) => {
            [surface.u_periodic(), surface.v_periodic()]
        }
        _ => [false, false],
    }
}

fn surface_parameter_bounds(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[Option<f64>; 4]>, CodecError> {
    fn visit(
        index: &ModelIndex<'_>,
        surface_id: &SurfaceId,
        visiting: &mut BTreeSet<SurfaceId>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Option<[Option<f64>; 4]>, CodecError> {
        let _nested = ctx.enter_nested("iges support-bound surface chain")?;
        if visiting.contains(surface_id) {
            return Ok(None);
        }
        let visited_id = copy_optional_identity(
            Some(ctx), surface_id.as_str(), "iges support-bound visiting surface ID",
        )?;
        insert_optional_btree_set(
            Some(ctx), visiting, visited_id, "iges support-bound visiting surface nodes",
        )?;
        let Some(procedural) = index.procedural_surface_for_surface(surface_id.as_str()) else {
            return Ok(None);
        };
        let bounds = match procedural.definition() {
            ProceduralSurfaceDefinition::Ruled { .. } => procedural
                .record_bounds()
                .map(RecordBounds::get)
                .map(|bounds| [bounds[0], bounds[1], Some(0.0), Some(1.0)]),
            ProceduralSurfaceDefinition::Extrusion(_) => procedural
                .record_bounds()
                .map(RecordBounds::get)
                .map(|bounds| [bounds[0], bounds[1], Some(0.0), Some(1.0)]),
            ProceduralSurfaceDefinition::Revolution(definition_payload) => {
                let angular_interval = definition_payload.angular_interval().endpoints();
                procedural
                    .record_bounds()
                    .map(RecordBounds::get)
                    .map(|bounds| {
                        [
                            bounds[0],
                            bounds[1],
                            Some(angular_interval[0]),
                            Some(angular_interval[1]),
                        ]
                    })
            }
            _ => procedural.record_bounds().map(RecordBounds::get),
        };
        if let Some(bounds) = bounds {
            return Ok(Some(bounds));
        }
        let support = match procedural.definition() {
            ProceduralSurfaceDefinition::Offset(definition_payload) => {
                let support = definition_payload.support();
                support
            }
            ProceduralSurfaceDefinition::ParallelOffset(definition_payload) => {
                let support = definition_payload.support();
                support
            }
            ProceduralSurfaceDefinition::Replica { source, .. } => source,
            _ => return Ok(None),
        };
        visit(index, support, visiting, ctx)
    }

    visit(index, surface_id, &mut BTreeSet::new(), ctx)
}

fn pcurves_agree(
    index: &ModelIndex<'_>,
    surface_id: &SurfaceId,
    pcurves: &[(PcurveGeometry, [f64; 2])],
    expected_start: Point3,
    expected_end: Point3,
    tolerance: f64,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut mapped = reserve_vec(ctx, pcurves.len(), "iges trimmed mapped pcurves")?;
    for (geometry, range) in pcurves {
        let Some(start_uv) = finite_or_refusal(cadmpeg_ir::eval::pcurve_uv(geometry, range[0]))?
        else {
            return Ok(false);
        };
        let Some(start) = finite_or_refusal(cadmpeg_ir::eval::model_surface_point_by_id(
            index, surface_id, start_uv.u, start_uv.v,
        ))?
        else {
            return Ok(false);
        };
        let Some(end_uv) = finite_or_refusal(cadmpeg_ir::eval::pcurve_uv(geometry, range[1]))?
        else {
            return Ok(false);
        };
        let Some(end) = finite_or_refusal(cadmpeg_ir::eval::model_surface_point_by_id(
            index, surface_id, end_uv.u, end_uv.v,
        ))?
        else {
            return Ok(false);
        };
        mapped.push((start.get(), end.get()));
    }
    Ok(mapped
        .first()
        .is_some_and(|(start, _)| close(*start, expected_start, tolerance))
        && mapped
            .last()
            .is_some_and(|(_, end)| close(*end, expected_end, tolerance))
        && mapped
            .windows(2)
            .all(|pair| close(pair[0].1, pair[1].0, tolerance)))
}

fn edge_range_matches_curve(
    edge: &Edge,
    carrier_index: &ModelIndex<'_>,
    start: Point3,
    end: Point3,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let Some(curve_id) = edge.curve() else {
        return Ok(false);
    };
    let Some(curve) = carrier_index.curves(curve_id.as_str()) else {
        return Ok(false);
    };
    let Some(range) = edge.param_range() else {
        return Ok(false);
    };
    if !range.iter().all(|parameter| parameter.is_finite()) {
        return Ok(false);
    }
    let geometry = &curve.geometry;
    let Some(evaluated_start) =
        finite_or_refusal(cadmpeg_ir::eval::curve_point(geometry, range[0]))?
    else {
        return Ok(false);
    };
    let Some(evaluated_end) = finite_or_refusal(cadmpeg_ir::eval::curve_point(geometry, range[1]))?
    else {
        return Ok(false);
    };
    Ok(
        close(evaluated_start.get(), start, tolerance)
            && close(evaluated_end.get(), end, tolerance),
    )
}

#[derive(Clone, Copy)]
struct BoundaryMatch<'a> {
    surface_id: &'a SurfaceId,
    pcurves: &'a [(PcurveGeometry, [f64; 2])],
    sense: Sense,
    tolerance: f64,
    parameter_curves_authoritative: bool,
}

fn select_boundary_edge(
    candidates: &[&Edge],
    carrier_index: &ModelIndex<'_>,
    boundary: BoundaryMatch<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<(Edge, FinitePoint3, FinitePoint3, bool), BoundaryEdgeSelectionError> {
    let BoundaryMatch {
        surface_id,
        pcurves,
        sense,
        tolerance,
        parameter_curves_authoritative,
    } = boundary;
    let mut candidates_with_endpoints = 0;
    let mut matched = reserve_vec(ctx, candidates.len(), "iges trimmed edge candidates")
        .map_err(BoundaryEdgeSelectionError::Resource)?;
    for &edge in candidates {
        let Some(start) = point_position(carrier_index, &edge.start) else {
            continue;
        };
        let Some(end) = point_position(carrier_index, &edge.end) else {
            continue;
        };
        candidates_with_endpoints += 1;
        if edge_range_matches_curve(edge, carrier_index, start.get(), end.get(), tolerance)
            .map_err(|limit| BoundaryEdgeSelectionError::Resource(limit.into()))?
        {
            matched.push((edge, start, end));
        }
    }
    let candidates = matched;
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
            Ok((clone_boundary_edge(edge, ctx).map_err(BoundaryEdgeSelectionError::Resource)?, start, end, true))
        } else {
            Err(BoundaryEdgeSelectionError::Ambiguous)
        };
    }

    let mut agreeing = reserve_vec(ctx, candidates.len(), "iges trimmed agreeing edges")
        .map_err(BoundaryEdgeSelectionError::Resource)?;
    for candidate @ (_, start, end) in &candidates {
        let (expected_start, expected_end) = if sense == Sense::Forward {
            (*start, *end)
        } else {
            (*end, *start)
        };
        if pcurves_agree(
            carrier_index,
            surface_id,
            pcurves,
            expected_start.get(),
            expected_end.get(),
            tolerance,
            ctx,
        )
        .map_err(BoundaryEdgeSelectionError::Resource)?
        {
            agreeing.push(candidate);
        }
    }
    match agreeing.as_slice() {
        [(edge, start, end)] => Ok((clone_boundary_edge(edge, ctx).map_err(BoundaryEdgeSelectionError::Resource)?, *start, *end, true)),
        [] if !parameter_curves_authoritative && candidates.len() == 1 => {
            let (edge, start, end) = candidates[0];
            Ok((clone_boundary_edge(edge, ctx).map_err(BoundaryEdgeSelectionError::Resource)?, start, end, false))
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

pub(super) fn clone_boundary_edge(edge: &Edge, ctx: &DecodeContext<'_>) -> Result<Edge, CodecError> {
    let carrier = match &edge.carrier {
        cadmpeg_ir::topology::EdgeCarrier::Free => cadmpeg_ir::topology::EdgeCarrier::Free,
        cadmpeg_ir::topology::EdgeCarrier::Endpoints(range) => cadmpeg_ir::topology::EdgeCarrier::Endpoints(*range),
        cadmpeg_ir::topology::EdgeCarrier::Curve(curve) => cadmpeg_ir::topology::EdgeCarrier::Curve(
            copy_optional_identity(Some(ctx), curve.as_str(), "iges selected edge curve ID")?,
        ),
        cadmpeg_ir::topology::EdgeCarrier::Bounded(curve, range) => cadmpeg_ir::topology::EdgeCarrier::Bounded(
            copy_optional_identity(Some(ctx), curve.as_str(), "iges selected edge curve ID")?, *range,
        ),
    };
    Ok(Edge {
        id: copy_optional_identity(Some(ctx), edge.id.as_str(), "iges selected edge ID")?,
        carrier,
        start: copy_optional_identity(Some(ctx), edge.start.as_str(), "iges selected edge start ID")?,
        end: copy_optional_identity(Some(ctx), edge.end.as_str(), "iges selected edge end ID")?,
        tolerance: edge.tolerance,
    })
}

pub(super) fn project(
    ir: &mut CadIr,
    directory: &[DirectoryEntry],
    parameters: &[ParameterRecord],
    global: &ProjectedGlobal,
    ctx: &DecodeContext<'_>,
    sequences: &mut super::geometry::SourceSequences,
) -> Result<(ProjectionOutcome, Vec<BoundaryVertexDerivation>), CodecError> {
    let mut records = BTreeMap::new();
    for record in parameters {
        crate::decode_resource::insert_optional_btree_map(
            Some(ctx), &mut records, record.directory_sequence, record,
            "iges trimming parameter index",
        )?;
    }
    let mut entries = BTreeMap::new();
    for entry in directory {
        crate::decode_resource::insert_optional_btree_map(
            Some(ctx), &mut entries, entry.sequence, entry,
            "iges trimming directory index",
        )?;
    }
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut boundary_vertex_derivations = Vec::new();
    let mut boundaries = BTreeMap::new();

    let carrier_index = ModelIndex::try_new_model_only_for_decode(ir, ctx)?;
    let mut composite_index: Option<CompositeIndex> = None;
    let mut edges_by_curve = BTreeMap::<&CurveId, Vec<&Edge>>::new();
    for edge in &ir.model.edges {
        if let Some(curve) = edge.curve() {
            if !edges_by_curve.contains_key(curve) {
                ctx.charge_collection_items(1, "iges boundary carrier index nodes")?;
            }
            let group = edges_by_curve.entry(curve).or_default();
            reserve_vec_growth(ctx, group, 1, "iges boundary carrier edge references")?;
            group.push(edge);
        }
    }
    let mut staged = Vec::new();
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 142 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "Parameter Data record is missing"))?;
            continue;
        };
        let Some(preference) = record
            .integer(5)
            .filter(|value| matches!(value, 0..=3) && matches!(record.integer(1), Some(0..=3)))
        else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "curve-on-surface creation or preference flag is invalid"))?;
            continue;
        };
        let Some(surface) = pointer(record, 2) else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "curve-on-surface surface pointer is invalid"))?;
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
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "curve-on-surface parameter curve pointer is invalid"))?;
            continue;
        }
        let Some(model_curve) = pointer(record, 4) else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "curve-on-surface model curve pointer is invalid"))?;
            continue;
        };
        if pcurve.is_some_and(|pcurve| {
            entries.get(&pcurve).is_none_or(|entry| {
                entry.status.use_flag(global.global_table()) != Some(UseFlag::Parametric)
            })
        }) {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "parameter curve does not have entity-use flag 05"))?;
            continue;
        }
        let mut pcurves = reserve_vec(ctx, usize::from(pcurve.is_some()), "iges Type142 boundary pcurve pointers")?;
        if let Some(pcurve) = pcurve {
            pcurves.push(pcurve);
        }
        let mut segments = reserve_vec(ctx, 1, "iges Type142 boundary segments")?;
        segments.push(BoundarySegment {
            pcurves,
            model_curve,
            sense: Sense::Forward,
            parameter_curves_authoritative: pcurve.is_some() && preference != 2,
        });
        crate::decode_resource::insert_optional_btree_map(
            Some(ctx), &mut boundaries,
            entry.sequence,
            BoundaryDefinition { surface, segments },
            "iges trimming boundary index nodes",
        )?;
        crate::decode_resource::insert_optional_btree_set(Some(ctx), &mut decoded, entry.sequence, "iges trimming decoded sequences")?;
    }
    for entry in directory
        .iter()
        .filter(|entry| entry.entity_type == 141 && entry.form == 0)
    {
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "Parameter Data record is missing"))?;
            continue;
        };
        let Some(boundary_type) = record.integer(1).filter(|value| matches!(value, 0 | 1)) else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary representation type is not 0 or 1"))?;
            continue;
        };
        let Some(preference) = record.integer(2).filter(|value| matches!(value, 0..=3)) else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary preference flag is invalid"))?;
            continue;
        };
        let Some(surface) = pointer(record, 3) else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary support pointer is invalid"))?;
            continue;
        };
        let Some(segment_count) = record.count(4).filter(|count| *count > 0) else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary segment count is not positive"))?;
            continue;
        };
        let mut index = 5;
        let mut segments = reserve_vec(ctx, segment_count, "iges Type141 boundary segments")?;
        let mut valid = true;
        for _ in 0..segment_count {
            let Some(model_curve) = pointer(record, index) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary model-curve pointer is invalid"))?;
                valid = false;
                break;
            };
            let sense = match record.integer(index + 1) {
                Some(1) => Sense::Forward,
                Some(2) => Sense::Reversed,
                _ => {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary segment sense is not 1 or 2"))?;
                    valid = false;
                    break;
                }
            };
            let Some(pcurve_count) = record.count(index + 2) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary pcurve count is invalid"))?;
                valid = false;
                break;
            };
            if (boundary_type == 0 && pcurve_count != 0)
                || (boundary_type == 1 && pcurve_count == 0)
            {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary pcurve collection cardinality disagrees with its representation type"))?;
                valid = false;
                break;
            }
            let mut pcurves = reserve_vec(ctx, pcurve_count, "iges Type141 segment pcurves")?;
            for pcurve_index in 0..pcurve_count {
                let Some(pcurve) = pointer(record, index + 3 + pcurve_index) else {
                    pcurves.clear();
                    break;
                };
                if entries.get(&pcurve).is_none_or(|entry| {
                    entry.status.use_flag(global.global_table()) != Some(UseFlag::Parametric)
                }) {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary pcurve does not have entity-use flag 05"))?;
                    pcurves.clear();
                    break;
                }
                pcurves.push(pcurve);
            }
            if pcurves.len() != pcurve_count {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary pcurve pointer is invalid"))?;
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
        crate::decode_resource::insert_optional_btree_map(
            Some(ctx), &mut boundaries, entry.sequence,
            BoundaryDefinition { surface, segments },
            "iges trimming boundary index nodes",
        )?;
            crate::decode_resource::insert_optional_btree_set(Some(ctx), &mut decoded, entry.sequence, "iges trimming decoded sequences")?;
        }
    }
    for entry in directory
        .iter()
        .filter(|entry| matches!(entry.entity_type, 143 | 144) && entry.form == 0)
    {
        let factor = global.length_factor_mm();
        let carrier_agreement_tolerance = global.minimum_resolution_mm();
        let Some(record) = records.get(&entry.sequence).copied() else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "Parameter Data record is missing"))?;
            continue;
        };
        let surface_kind = if entry.entity_type == 144 {
            BoundarySurfaceKind::Trimmed
        } else {
            BoundarySurfaceKind::Bounded
        };
        let (
            surface_sequence,
            boundary_sequences,
            has_explicit_outer,
            explicit_outer_sequence,
            mut valid,
        ) = if surface_kind == BoundarySurfaceKind::Trimmed {
            let Some(surface) = pointer(record, 1) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface support pointer is invalid"))?;
                continue;
            };
            let Some(has_explicit_outer) = record.integer(2).and_then(|value| match value {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            }) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface outer-boundary flag is not 0 or 1"))?;
                continue;
            };
            let Some(inner_count) = record.count(3) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface inner-boundary count is invalid"))?;
                continue;
            };
            let sequence_count = inner_count.checked_add(usize::from(has_explicit_outer))
                .ok_or_else(|| ctx.refuse_codec_limit("iges Type144 boundary sequences", u64::MAX, 1))?;
            let mut sequences = reserve_vec(ctx, sequence_count, "iges Type144 boundary sequences")?;
            // The outer boundary is stated in its own PTO field, so it travels
            // as its own value and is never recovered from a list position.
            let mut explicit_outer_sequence = None;
            if has_explicit_outer {
                let Some(outer) = pointer(record, 4) else {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface outer-boundary pointer is invalid"))?;
                    continue;
                };
                if entries
                    .get(&outer)
                    .is_none_or(|target| target.entity_type != 142 || target.form != 0)
                {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface outer-boundary pointer does not target a Type 142 Form 0 entity"))?;
                    continue;
                }
                sequences.push(outer);
                explicit_outer_sequence = Some(outer);
            } else if !matches!(
                record.value(4),
                None | Some(TokenValue::Omitted | TokenValue::Integer(0))
            ) {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface parameter-domain outer-boundary pointer is neither zero nor omitted"))?;
                continue;
            }
            let mut valid = true;
            for index in 0..inner_count {
                let Some(sequence) = pointer(record, 5 + index) else {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface inner-boundary pointer is invalid"))?;
                    valid = false;
                    break;
                };
                if entries
                    .get(&sequence)
                    .is_none_or(|target| target.entity_type != 142 || target.form != 0)
                {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface inner-boundary pointer does not target a Type 142 Form 0 entity"))?;
                    valid = false;
                    break;
                }
                sequences.push(sequence);
            }
            (
                surface,
                sequences,
                has_explicit_outer,
                explicit_outer_sequence,
                valid,
            )
        } else {
            let Some(representation) = record.integer(1).filter(|value| matches!(value, 0 | 1))
            else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "bounded-surface representation type is not 0 or 1"))?;
                continue;
            };
            let Some(surface) = pointer(record, 2) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "bounded-surface support pointer is invalid"))?;
                continue;
            };
            let Some(count) = record.count(3).filter(|count| *count > 0) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "bounded-surface boundary count is not positive"))?;
                continue;
            };
            let mut sequences = reserve_vec(ctx, count, "iges Type143 boundary sequences")?;
            let mut valid = true;
            for index in 0..count {
                let Some(sequence) = pointer(record, 4 + index) else {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "bounded-surface boundary pointer is invalid"))?;
                    valid = false;
                    break;
                };
                if entries
                    .get(&sequence)
                    .is_none_or(|target| target.entity_type != 141 || target.form != 0)
                {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "bounded-surface boundary pointer does not target a Type 141 Form 0 entity"))?;
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
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "bounded-surface representation disagrees with its boundary"))?;
                    valid = false;
                    break;
                }
            }
            (surface, sequences, false, None, valid)
        };
        if !valid {
            continue;
        }
        let surface_id = crate::ids::surface(&crate::ids::Stem::directory(surface_sequence));
        let Some(support_geometry) = carrier_index.surfaces(surface_id.as_str()).map(|surface| {
            surface.geometry.solved_cache().map_or_else(
                || surface.geometry.clone(),
                |cache| SurfaceGeometry::Solved(cache.clone()),
            )
        }) else {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface support carrier is missing"))?;
            continue;
        };
        let mut candidate = ModelDraft::new();
        let stem = crate::ids::Stem::directory(entry.sequence);
        let body_id = crate::ids::body(&stem);
        sequences.record_body(&body_id, entry.sequence, &stem, Some(ctx))?;
        let region_id = crate::ids::region(&stem);
        let shell_id = crate::ids::shell(&stem);
        let face_id = crate::ids::face(&stem);
        sequences.record_face(&face_id, entry.sequence, Some(ctx))?;
        let mut candidate_boundary_vertex_derivations = Vec::new();
        let support_parameter_bounds = surface_parameter_bounds(&carrier_index, &surface_id, ctx)?;
        let support_parameter_intervals = surface_parameter_bound_intervals(
            support_parameter_bounds,
            &surface_id,
            &entries,
            &records,
            global.real_precision(),
        );
        let periodic_parameters = periodic_surface_parameters(&support_geometry);
        let implicit_outer_domain = surface_kind == BoundarySurfaceKind::Trimmed
            && !has_explicit_outer
            && !boundary_sequences.is_empty();
        let mut implicit_boundary_curves = Vec::new();
        let mut implicit_boundary_pcurves = Vec::new();
        let mut loop_ids = Vec::new();
        let mut explicit_outer_loop: Option<cadmpeg_ir::ids::LoopId> = None;
        let mut linear_boundary_candidates = reserve_vec(ctx, boundary_sequences.len(), "iges trimming linear candidates")?;
        let mut face_tolerance = 0.0_f64;
        for (boundary_index, sequence) in boundary_sequences.iter().copied().enumerate() {
            let Some(boundary) = boundaries.get(&sequence) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed-surface boundary definition is missing"))?;
                valid = false;
                break;
            };
            if boundary.surface != surface_sequence {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary definition names a different support surface"))?;
                valid = false;
                break;
            }
            let mut items = reserve_vec(ctx, boundary.segments.len(), "iges trimming boundary items")?;
            for segment in &boundary.segments {
                let model_curve_id =
                    crate::ids::curve(&crate::ids::Stem::directory(segment.model_curve));
                let Some(candidates) = edges_by_curve.get(&model_curve_id) else {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary model curve has no bounded edge"))?;
                    valid = false;
                    break;
                };
                let mut pcurves = Some(reserve_vec(ctx, segment.pcurves.len(), "iges trimming segment pcurves")?);
                let mut pcurve_refusal = None;
                for sequence in &segment.pcurves {
                    if composite_index.is_none() {
                        composite_index = Some(CompositeIndex::from_ir(ir, Some(ctx))?);
                    }
                    let index = composite_index.as_ref().ok_or_else(|| CodecError::Malformed("IGES trimming composite index is absent".into()))?;
                    match pcurve_geometry(
                        ir,
                        *sequence,
                        &PcurveSupport {
                            surface_id: &surface_id,
                            geometry: &support_geometry,
                            factor,
                        },
                        Some(carrier_agreement_tolerance),
                        Some(ctx),
                        Some(index),
                    ) {
                        Ok(Some(resolved)) => {
                            if let Some(pcurves) = pcurves.as_mut() {
                                pcurves.push(resolved);
                            }
                        }
                        Ok(None) => {
                            pcurves = None;
                            break;
                        }
                        Err(error) => {
                            pcurves = None;
                            pcurve_refusal = Some(error);
                            break;
                        }
                    }
                }
                if let Some(error) = pcurve_refusal {
                    let error = error.non_resource()?;
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("boundary parameter curve states no NURBS carrier: {error}"))?;
                    valid = false;
                    break;
                }
                let mut pcurves = match pcurves {
                    Some(pcurves) => pcurves,
                    None if segment.parameter_curves_authoritative => {
                        super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary parameter curve has no NURBS carrier"))?;
                        valid = false;
                        break;
                    }
                    None => Vec::new(),
                };
                let mut pcurve_outside_support = false;
                for ((geometry, range), sequence) in pcurves.iter().zip(&segment.pcurves) {
                    if !pcurve_within_declared_intervals(
                            geometry,
                            *range,
                            support_parameter_intervals,
                            periodic_parameters,
                            ctx,
                        )? && !source_curve_control_polygon_within_bounds(
                            ir,
                            &crate::ids::curve(&crate::ids::Stem::directory(*sequence)),
                            &PcurveSupport {
                                surface_id: &surface_id,
                                geometry: &support_geometry,
                                factor,
                            },
                            support_parameter_intervals,
                            &entries,
                            &records,
                            global.real_precision(),
                            ctx,
                        )? {
                        pcurve_outside_support = true;
                        break;
                    }
                }
                if pcurve_outside_support {
                    if segment.parameter_curves_authoritative {
                        super::push_attributed_loss(ctx, &mut losses, entry,
                            IgesLossCode::BoundaryPcurveOutsideSupportDomain,
                            format_args!("IGES entity type {} form {}: boundary parameter curve leaves the declared support parameter bounds", entry.entity_type, entry.form),
                        )?;
                        valid = false;
                        break;
                    }
                    super::push_attributed_loss(ctx, &mut losses, entry,
                        IgesLossCode::BoundaryPcurveOutsideSupportDomain,
                        format_args!("IGES entity type {} form {}: alternate boundary parameter curve leaves the declared support parameter bounds; model-space curve retained", entry.entity_type, entry.form),
                    )?;
                    pcurves.clear();
                }
                let (source_edge, start, end, pcurves_agree) = match select_boundary_edge(
                    candidates,
                    &carrier_index,
                    BoundaryMatch {
                        surface_id: &surface_id,
                        pcurves: &pcurves,
                        sense: segment.sense,
                        tolerance: carrier_agreement_tolerance,
                        parameter_curves_authoritative: segment.parameter_curves_authoritative,
                    },
                    ctx,
                ) {
                    Ok(selected) => selected,
                    Err(BoundaryEdgeSelectionError::MissingEndpoints) => {
                        super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary model-curve endpoints are missing"))?;
                        valid = false;
                        break;
                    }
                    Err(BoundaryEdgeSelectionError::InvalidRange) => {
                        super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary model-curve edge range does not evaluate to its vertices"))?;
                        valid = false;
                        break;
                    }
                    Err(BoundaryEdgeSelectionError::Ambiguous) => {
                        super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary model curve maps to multiple ambiguous edge occurrences"))?;
                        valid = false;
                        break;
                    }
                    Err(BoundaryEdgeSelectionError::PcurveDisagreement) => {
                        super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "curve-on-surface carriers disagree beyond the minimum resolution"))?;
                        valid = false;
                        break;
                    }
                    Err(BoundaryEdgeSelectionError::Resource(error)) => return Err(error),
                };
                if !pcurves_agree {
                    pcurves.clear();
                }
                items.push(BoundaryItem {
                    segment,
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
                reserve_vec_growth(ctx, &mut implicit_boundary_curves, items.len(), "iges implicit boundary curve IDs")?;
                for item in &items {
                    implicit_boundary_curves.push(copy_optional_identity(
                        Some(ctx), item.model_curve.as_str(), "iges implicit boundary curve ID text",
                    )?);
                }
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
                items
                    .iter()
                    .flat_map(|item| [item.start.get(), item.end.get()]),
            );
            let sewing_tolerance = tolerance_policy.topology_sewing;
            face_tolerance = face_tolerance.max(sewing_tolerance);
            if items.iter().enumerate().any(|(index, item)| {
                let (_, end) = traversal(item);
                let (next_start, _) = traversal(&items[(index + 1) % items.len()]);
                !close(end.get(), next_start.get(), sewing_tolerance)
            }) {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "ordered boundary segments do not form a closed ring"))?;
                valid = false;
                break;
            }
            linear_boundary_candidates.push(linear_boundary_geometry(
                &items,
                &carrier_index,
                &support_geometry,
                carrier_agreement_tolerance,
                sewing_tolerance,
                surface_kind,
                ctx,
            )?);
            let loop_id = crate::ids::r#loop(&stem.slot(boundary_index));
            let mut coedge_ids = reserve_vec(ctx, items.len(), "iges trimming coedge ids")?;
            let endpoint_count = items.len().checked_mul(2).ok_or_else(|| cadmpeg_core::decode::refuse_local_limit("iges trimming source endpoints", u64::MAX, 1))?;
            let mut source_endpoints = reserve_vec(ctx, endpoint_count, "iges trimming source endpoints")?;
            for (index, item) in items.iter().enumerate() {
                coedge_ids.push(crate::ids::coedge(&stem.slot(boundary_index).slot(index)));
                for (endpoint, position) in [(BoundaryEndpoint::Start, item.start), (BoundaryEndpoint::End, item.end)] {
                    source_endpoints.push(BoundaryVertexSourceEndpoint {
                        edge: format_retained(ctx, format_args!("{}", item.source_edge.id), "iges trimming source endpoint edge text")?,
                        endpoint,
                        position,
                    });
                }
            }
            let Some(checked_sewing_tolerance) =
                cadmpeg_ir::scalar::PositiveReal::new(sewing_tolerance)
            else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary sewing tolerance is invalid"))?;
                valid = false;
                break;
            };
            let (vertex_ids, derivations) = match create_boundary_vertices(
                &mut candidate,
                &stem,
                &format_retained(ctx, format_args!("iges:entity:directory#{}", entry.sequence), "iges trimming source entity text")?,
                boundary_index,
                &source_endpoints,
                checked_sewing_tolerance,
                sequences,
                ctx,
            ) {
                Ok(result) => result,
                Err(BoundaryVertexCreationError::Cluster(BoundaryVertexClusterError::NonTransitive)) => {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary endpoint tolerance neighborhoods are non-transitive"))?;
                    valid = false;
                    break;
                }
                Err(BoundaryVertexCreationError::Resource(error)) => return Err(error),
            };
            reserve_vec_growth(ctx, &mut candidate_boundary_vertex_derivations, derivations.len(), "iges trimming candidate vertex derivations")?;
            candidate_boundary_vertex_derivations.extend(derivations);
            for (segment_index, item) in items.into_iter().enumerate() {
                let edge_id = crate::ids::edge(&stem.slot(boundary_index).slot(segment_index));
                let start_vertex = crate::decode_resource::clone_optional_identity(
                    Some(ctx), &vertex_ids[segment_index * 2], "iges trimming identity copy",
                )?;
                let end_vertex = crate::decode_resource::clone_optional_identity(
                    Some(ctx), &vertex_ids[segment_index * 2 + 1], "iges trimming identity copy",
                )?;
                let carrier = match cadmpeg_ir::topology::EdgeCarrier::new(
                    Some(item.model_curve),
                    item.source_edge
                        .param_range()
                        .map(cadmpeg_ir::units::FiniteVector::get),
                ) {
                    Ok(carrier) => carrier,
                    Err(error) => {
                        super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", error))?;
                        valid = false;
                        break;
                    }
                };
                crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
                candidate.model_mut().edges.push(Edge {
                    id: crate::decode_resource::clone_optional_identity(Some(ctx), &edge_id, "iges trimming identity copy")?,
                    carrier,
                    start: start_vertex,
                    end: end_vertex,
                    tolerance: Some(checked_sewing_tolerance),
                });
                if item.pcurves.iter().any(|(_, range)|
                    cadmpeg_ir::units::FiniteVector::new(*range).is_none()
                ) {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry,
                        format_args!("{}", PcurveMetadata::NON_FINITE_PARAMETER_RANGE))?;
                    valid = false;
                    break;
                }
                let mut pcurve_uses = reserve_vec(ctx, item.pcurves.len(), "iges trimming coedge pcurve uses")?;
                for (pcurve_index, (geometry, parameter_range)) in item.pcurves.into_iter().enumerate() {
                    let id = crate::ids::pcurve(
                        &stem.slot(boundary_index).slot(segment_index).slot(pcurve_index),
                    );
                    if implicit_outer_domain {
                        reserve_vec_growth(ctx, &mut implicit_boundary_pcurves, 1, "iges implicit boundary pcurve IDs")?;
                        implicit_boundary_pcurves.push(copy_optional_identity(
                            Some(ctx), id.as_str(), "iges implicit boundary pcurve ID text",
                        )?);
                    }
                    reserve_vec_growth(ctx, &mut candidate.model_mut().pcurves, 1, "iges trimming pcurve slots")?;
                    crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
                    candidate.model_mut().pcurves.push(Pcurve {
                        id: copy_optional_identity(Some(ctx), id.as_str(), "iges trimming pcurve ID copy")?,
                        geometry,
                        metadata: PcurveMetadata::general(
                            None,
                            cadmpeg_ir::units::FiniteVector::new(parameter_range),
                            None,
                        ),
                    });
                    pcurve_uses.push(PcurveUse {
                        pcurve: id,
                        isoparametric: None,
                        parameter_range: None,
                    });
                }
                let coedge_id = crate::decode_resource::clone_optional_identity(
                    Some(ctx), &coedge_ids[segment_index], "iges trimming identity copy",
                )?;
                crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
                candidate.model_mut().coedges.push(Coedge {
                    id: crate::decode_resource::clone_optional_identity(Some(ctx), &coedge_id, "iges trimming identity copy")?,
                    owner_loop: crate::decode_resource::clone_optional_identity(Some(ctx), &loop_id, "iges trimming identity copy")?,
                    edge: edge_id,
                    radial_next: coedge_id,
                    sense: item.segment.sense,
                    pcurves: pcurve_uses,
                    use_curve: None,
                });
            }
            let ring = match cadmpeg_ir::topology::LoopRing::new_admitted(coedge_ids, Vec::new(), ctx) {
                Ok(ring) => ring,
                Err(cadmpeg_ir::topology::LoopRingAdmissionError::Invalid(_)) => {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "boundary loop contains no coedges"))?;
                    valid = false;
                    break;
                }
                Err(cadmpeg_ir::topology::LoopRingAdmissionError::Resource(error)) => return Err(error),
            };
            crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
            candidate.model_mut().loops.push(Loop {
                id: crate::decode_resource::clone_optional_identity(Some(ctx), &loop_id, "iges trimming identity copy")?,
                face: crate::decode_resource::clone_optional_identity(Some(ctx), &face_id, "iges trimming identity copy")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
            if explicit_outer_sequence == Some(sequence) {
                explicit_outer_loop = Some(crate::decode_resource::clone_optional_identity(Some(ctx), &loop_id, "iges trimming identity copy")?);
            }
            reserve_vec_growth(ctx, &mut loop_ids, 1, "iges trimming face loop IDs")?;
            loop_ids.push(loop_id);
        }
        if !valid {
            continue;
        }
        let linear_rings = match linear_boundary_rings(&linear_boundary_candidates, BoundarySpace::Parameter, ctx)? {
            Some(rings) => Some(rings),
            None => linear_boundary_rings(&linear_boundary_candidates, BoundarySpace::Model, ctx)?,
        };
        let linear_relationship = linear_rings.and_then(|rings| {
            linear_boundary_relationship_is_valid(
                rings.as_deref(),
                surface_kind,
                has_explicit_outer,
                &support_geometry,
                support_parameter_bounds,
                periodic_parameters,
            )
        });
        if linear_relationship == Some(false) {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", if surface_kind == BoundarySurfaceKind::Trimmed {
                    "trimmed-surface boundary loops are not simple, disjoint, and correctly nested"
                } else {
                    "boundary loop is not a simple closed carrier"
                }))?;
            continue;
        }
        let face_surface_id = if implicit_outer_domain {
            let derived_surface_id = crate::ids::surface(
                &crate::ids::Stem::directory(entry.sequence).part(crate::ids::Word::ImplicitOuter),
            );
            sequences.record_surface(&derived_surface_id, entry.sequence, Some(ctx))?;
            crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
            candidate.model_mut().surfaces.push(Surface {
                id: crate::decode_resource::clone_optional_identity(Some(ctx), &derived_surface_id, "iges trimming identity copy")?,
                geometry: support_geometry,
                source_object: Some(match source_object(entry, Some(ctx)) {
                    Ok(source) => source,
                    Err(error) => {
                        super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", super::non_resource_error(error)?))?;
                        continue;
                    }
                }),
            });
            let record_bounds = match support_parameter_bounds
                .map(RecordBounds::try_new)
                .transpose()
            {
                Ok(record_bounds) => record_bounds,
                Err(error) => {
                    super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", error.to_string()))?;
                    continue;
                }
            };
            reserve_vec_growth(ctx, &mut candidate.model_mut().procedural_surfaces, 1, "iges procedural surface slots")?;
            crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
            let _attached = candidate.model_mut().add_procedural_surface(
                crate::decode_resource::clone_optional_identity(Some(ctx), &derived_surface_id, "iges trimming identity copy")?,
                ProceduralSurface::new(
                    crate::ids::procedural_surface(
                        &crate::ids::Stem::directory(entry.sequence)
                            .part(crate::ids::Word::ImplicitOuter),
                    ),
                    ProceduralSurfaceDefinition::CurveBounded {
                        support: crate::decode_resource::clone_optional_identity(Some(ctx), &surface_id, "iges trimming identity copy")?,
                        boundaries: implicit_boundary_curves,
                        boundary_pcurves: implicit_boundary_pcurves,
                        implicit_outer: true,
                    },
                    record_bounds,
                ),
            );
            derived_surface_id
        } else {
            surface_id
        };
        let checked_face_tolerance = if face_tolerance > 0.0 {
            let Some(value) = cadmpeg_ir::scalar::PositiveReal::new(face_tolerance) else {
                super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "face tolerance is invalid"))?;
                continue;
            };
            Some(value)
        } else {
            None
        };
        let face_loops = match explicit_outer_loop {
            Some(outer) => {
                let inner_count = loop_ids.iter().filter(|id| **id != outer).count();
                let mut inner = reserve_vec(ctx, inner_count, "iges trimming inner loop IDs")?;
                inner.extend(loop_ids.into_iter().filter(|id| *id != outer));
                cadmpeg_ir::topology::FaceLoops::classified(outer, inner)
            }
            None => cadmpeg_ir::topology::FaceLoops::unspecified(loop_ids),
        };
        crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
        candidate.model_mut().faces.push(Face {
            id: crate::decode_resource::clone_optional_identity(Some(ctx), &face_id, "iges trimming identity copy")?,
            shell: crate::decode_resource::clone_optional_identity(Some(ctx), &shell_id, "iges trimming identity copy")?,
            surface: face_surface_id,
            sense: Sense::Forward,
            loops: face_loops,
            name: None,
            color: None,
            tolerance: checked_face_tolerance,
        });
        let mut shell_faces = reserve_vec(ctx, 1, "iges trimming shell face IDs")?;
        shell_faces.push(face_id);
        crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
        candidate.model_mut().shells.push(Shell::new(
            crate::decode_resource::clone_optional_identity(Some(ctx), &shell_id, "iges trimming identity copy")?, crate::decode_resource::clone_optional_identity(Some(ctx), &region_id, "iges trimming identity copy")?, shell_faces, Vec::new(), Vec::new(),
        ).map_err(|error| CodecError::Malformed(error.to_string()))?);
        let mut region_shells = reserve_vec(ctx, 1, "iges trimming region shell IDs")?;
        region_shells.push(shell_id);
        crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
        candidate.model_mut().regions.push(Region {
            id: crate::decode_resource::clone_optional_identity(Some(ctx), &region_id, "iges trimming identity copy")?,
            body: crate::decode_resource::clone_optional_identity(Some(ctx), &body_id, "iges trimming identity copy")?,
            shells: region_shells,
        });
        let mut body_regions = reserve_vec(ctx, 1, "iges trimming body region IDs")?;
        body_regions.push(region_id);
        crate::decode_resource::admit_optional_entities(Some(ctx), 1, "iges_geometry_trimming")?;
        candidate.model_mut().bodies.push(Body {
            id: body_id,
            kind: BodyKind::Sheet,
            regions: body_regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        candidate.model_mut().finalize();
        reserve_vec_growth(ctx, &mut staged, 1, "iges trimming staged candidates")?;
        staged.push((entry, candidate, candidate_boundary_vertex_derivations));
    }
    drop(carrier_index);
    let mut commit_session = CommitSession::new(ir);
    for (entry, candidate, derivations) in staged {
        if commit_session.commit_model_admitted(candidate, ctx)?.is_err() {
            super::push_optional_entity_loss(Some(ctx), &mut losses, entry, format_args!("{}", "trimmed sheet candidate failed neutral validation"))?;
            continue;
        }
        crate::decode_resource::insert_optional_btree_set(Some(ctx), &mut decoded, entry.sequence, "iges trimming decoded sequences")?;
        reserve_vec_growth(ctx, &mut boundary_vertex_derivations, derivations.len(), "iges trimming committed vertex derivations")?;
        boundary_vertex_derivations.extend(derivations);
    }

    Ok((
        ProjectionOutcome { decoded, losses },
        boundary_vertex_derivations,
    ))
}

#[cfg(test)]
mod tests;
