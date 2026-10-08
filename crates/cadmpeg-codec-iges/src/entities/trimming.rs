// SPDX-License-Identifier: Apache-2.0
//! Face-local trimmed-surface projection.

use super::composite::{bounded_nurbs_for_curve_with_tolerance, CompositeIndex};
use super::geometry::{
    linear_nurbs_parameters, planar_polyline_has_self_intersection, planar_polylines_intersect,
    plane_coordinates, source_object, BoundaryEndpoint, BoundaryVertexDerivation,
    BoundaryVertexSourceEndpoint, DeclaredInterval, ProjectionOutcome,
};
use super::{affine_parameter_map, pointer};
use crate::directory::{entry_by_sequence, DirectoryEntry, UseFlag};
use crate::global::{ProjectedGlobal, RealPrecision};
use crate::loss::IgesLossCode;
use crate::parameter::{record_by_sequence, ParameterRecord, TokenValue};
use cadmpeg_core::decode::{DecodeContext, ScopedReservation};
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
use cadmpeg_ir::ids::{CurveId, SurfaceId, VertexId};
use cadmpeg_ir::index::{DecodeModelIndex, ModelIndex};
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

impl From<CodecError> for BoundaryEdgeSelectionError {
    fn from(error: CodecError) -> Self {
        Self::Resource(error)
    }
}

impl From<cadmpeg_core::decode::ResourceLimit> for BoundaryVertexCreationError {
    fn from(limit: cadmpeg_core::decode::ResourceLimit) -> Self {
        Self::Resource(limit.into())
    }
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
    fn from_global(
        global: &ProjectedGlobal,
        points: impl Iterator<Item = Point3>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Self, CodecError> {
        let carrier_agreement = global.minimum_resolution_mm();
        let coordinate_quantum = coordinate_quantum(global, points, ctx)?;
        Ok(Self {
            topology_sewing: carrier_agreement.max(coordinate_quantum),
        })
    }
}

fn coordinate_quantum(
    global: &ProjectedGlobal,
    mut points: impl Iterator<Item = Point3>,
    ctx: &DecodeContext<'_>,
) -> Result<f64, CodecError> {
    let mut magnitude = 0.0_f64;
    while let Some(point) = ctx.next_charged(&mut points, "iges face coordinate magnitude")? {
        magnitude = magnitude
            .max(point.x.abs())
            .max(point.y.abs())
            .max(point.z.abs());
    }
    Ok(if magnitude > 0.0 {
        10.0_f64.powf(
            magnitude.log10().floor() - f64::from(global.single_precision_significance()) + 1.0,
        )
    } else {
        0.0
    })
}

fn point_order(left: Point3, right: Point3) -> Ordering {
    left.x
        .total_cmp(&right.x)
        .then_with(|| left.y.total_cmp(&right.y))
        .then_with(|| left.z.total_cmp(&right.z))
}

fn find_cluster_root(
    parents: &mut [usize],
    index: usize,
    ctx: &DecodeContext<'_>,
) -> Result<usize, CodecError> {
    let mut root = index;
    let mut steps = std::iter::repeat(());
    loop {
        ctx.next_charged(&mut steps, "iges boundary cluster root traversal")?;
        let parent = *parents
            .get(root)
            .ok_or_else(|| CodecError::malformed("boundary cluster parent is out of range"))?;
        if parent == root {
            break;
        }
        root = parent;
    }
    let mut current = index;
    while current != root {
        ctx.next_charged(&mut steps, "iges boundary cluster path compression")?;
        let parent = parents
            .get_mut(current)
            .ok_or_else(|| CodecError::malformed("boundary cluster parent is out of range"))?;
        let next = *parent;
        *parent = root;
        current = next;
    }
    Ok(root)
}

fn cluster_boundary_positions(
    positions: &[FinitePoint3],
    tolerance: cadmpeg_ir::scalar::PositiveReal,
    ctx: &DecodeContext<'_>,
) -> Result<Vec<BoundaryVertexCluster>, BoundaryVertexCreationError> {
    let tolerance = tolerance.get();
    let (mut parents, _parent_storage) =
        ctx.temporary_vec(positions.len(), "iges boundary cluster parents")?;
    parents.extend(ctx.admit_iter(0..positions.len(), "iges boundary cluster initialization")?);
    let mut size_storage = ctx.reserve_scoped(0, "iges boundary cluster sizes")?;
    let mut sizes = size_storage.with_storage(|| {
        ctx.alloc_filled(positions.len(), 1usize, "iges boundary cluster sizes")
    })?;
    for (left_index, left) in ctx
        .admit_iter(positions, "iges boundary clustering positions")?
        .enumerate()
    {
        for (offset, right) in ctx
            .admit_iter(
                &positions[left_index + 1..],
                "iges boundary clustering comparisons",
            )?
            .enumerate()
        {
            let right_index = left_index + 1 + offset;
            if !close(left.get(), right.get(), tolerance) {
                continue;
            }
            let mut left_root = find_cluster_root(&mut parents, left_index, ctx)?;
            let mut right_root = find_cluster_root(&mut parents, right_index, ctx)?;
            if left_root != right_root {
                if sizes[left_root] < sizes[right_root] {
                    std::mem::swap(&mut left_root, &mut right_root);
                }
                sizes[left_root] =
                    sizes[left_root]
                        .checked_add(sizes[right_root])
                        .ok_or_else(|| {
                            ctx.refuse_codec_limit("iges boundary cluster size", u64::MAX, u64::MAX)
                        })?;
                parents[right_root] = left_root;
            }
        }
    }
    let mut members_by_root = BTreeMap::<usize, Vec<usize>>::new();
    let mut root_storage = ctx.reserve_scoped(0, "iges boundary cluster roots")?;
    for index in ctx.admit_iter(
        0..positions.len(),
        "iges boundary cluster membership traversal",
    )? {
        let root = find_cluster_root(&mut parents, index, ctx)?;
        root_storage.with_storage(|| {
            ctx.admit_btree_entry(&members_by_root, &root, "iges boundary cluster roots")
        })?;
        let members = members_by_root.entry(root).or_default();
        ctx.reserve_vec(members, 1, "iges boundary cluster members")?;
        members.push(index);
    }
    let mut clusters = ctx.collection_vec(members_by_root.len(), "iges boundary cluster slots")?;
    for (_, members) in ctx.admit_iter(members_by_root, "iges boundary cluster groups")? {
        if ctx.any_by(
            members.iter().enumerate(),
            |(offset, left)| {
                ctx.any_by(
                    &members[offset + 1..],
                    |right| {
                        Ok(!close(
                            positions[*left].get(),
                            positions[*right].get(),
                            tolerance,
                        ))
                    },
                    "iges boundary cluster transitivity comparisons",
                )
            },
            "iges boundary cluster transitivity members",
        )? {
            return Err(BoundaryVertexClusterError::NonTransitive.into());
        }
        let representative = ctx
            .min_by(
                &members,
                |left, right| {
                    Ok(point_order(positions[*left].get(), positions[*right].get())
                        .then_with(|| left.cmp(right)))
                },
                "iges boundary cluster representative comparisons",
            )?
            .map(|index| positions[*index])
            .ok_or(BoundaryVertexClusterError::NonTransitive)?;
        clusters.push(BoundaryVertexCluster {
            representative,
            members,
        });
    }
    ctx.stable_sort_by_key(
        &mut clusters,
        |value| value.members[0],
        Ord::cmp,
        "iges boundary clusters sort",
    )?;
    Ok(clusters)
}

#[derive(Debug)]
struct BoundaryVertices<'ctx> {
    ids: Vec<VertexId>,
    derivations: Vec<BoundaryVertexDerivation>,
    _storage: ScopedReservation<'ctx>,
}

fn create_boundary_vertices<'ctx>(
    candidate: &mut ModelDraft,
    stem: &crate::ids::Stem,
    source: (&str, usize),
    source_endpoints: &[BoundaryVertexSourceEndpoint],
    tolerance: cadmpeg_ir::scalar::PositiveReal,
    sequences: &mut super::geometry::SourceSequences,
    ctx: &'ctx DecodeContext<'_>,
) -> Result<BoundaryVertices<'ctx>, BoundaryVertexCreationError> {
    let (source_entity, boundary) = source;
    let (mut positions, _position_storage) =
        ctx.temporary_vec(source_endpoints.len(), "iges boundary endpoint positions")?;
    positions.extend(
        ctx.admit_iter(source_endpoints, "iges boundary endpoint position copy")?
            .map(|endpoint| endpoint.position),
    );
    let mut cluster_storage = ctx.reserve_scoped(0, "iges boundary clustering storage")?;
    let clusters =
        cluster_storage.with_storage(|| cluster_boundary_positions(&positions, tolerance, ctx))?;
    let mut vertex_storage = ctx.reserve_scoped(0, "iges boundary endpoint vertex storage")?;
    let mut vertex_ids = vertex_storage.with_storage(|| {
        ctx.collect_indexed_vec(
            positions.len(),
            "iges boundary endpoint vertex slots",
            |_| Ok(None),
        )
    })?;
    let mut derivations = ctx.collection_vec(clusters.len(), "iges boundary vertex derivations")?;
    for (index, cluster) in ctx
        .admit_iter(clusters, "iges boundary vertex cluster traversal")?
        .enumerate()
    {
        let point_id = crate::ids::point_admitted(&stem.slot(boundary).slot(index), ctx)?;
        ctx.reserve_vec(&mut candidate.model_mut().points, 1, "iges boundary points")?;
        ctx.reserve_vec(
            &mut candidate.model_mut().vertices,
            1,
            "iges boundary vertices",
        )?;
        sequences.record_point(&point_id, stem, ctx)?;
        let vertex_id = vertex_storage
            .with_storage(|| crate::ids::vertex_admitted(&stem.slot(boundary).slot(index), ctx))?;
        ctx.charge_entities(1, "iges_geometry_trimming")?;
        candidate.model_mut().points.push(Point::new(
            point_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            cluster.representative,
            None,
        ));
        ctx.charge_entities(1, "iges_geometry_trimming")?;
        candidate.model_mut().vertices.push(Vertex {
            id: vertex_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            point: point_id,
            tolerance: Some(tolerance),
        });
        let mut derivation_endpoints =
            ctx.collection_vec(cluster.members.len(), "iges boundary derivation endpoints")?;
        for member in ctx.admit_iter(
            &cluster.members,
            "iges boundary derivation member traversal",
        )? {
            let endpoint = &source_endpoints[*member];
            derivation_endpoints.push(BoundaryVertexSourceEndpoint {
                edge: ctx.format_retained(
                    format_args!("{}", endpoint.edge),
                    "iges boundary derivation edge text",
                )?,
                endpoint: endpoint.endpoint,
                position: endpoint.position,
            });
        }
        derivations.push(BoundaryVertexDerivation {
            source_entity: ctx.format_retained(
                format_args!("{source_entity}"),
                "iges boundary derivation source text",
            )?,
            vertex: vertex_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            representative: cluster.representative,
            tolerance: tolerance.get(),
            source_endpoints: derivation_endpoints,
        });
        for member in ctx.admit_iter(cluster.members, "iges boundary vertex member traversal")? {
            vertex_ids[member] = Some(vertex_storage.with_storage(|| {
                vertex_id.try_clone_for_decode(ctx, "iges trimming identity copy")
            })?);
        }
    }
    let mut result_ids = vertex_storage
        .with_storage(|| ctx.collection_vec(vertex_ids.len(), "iges boundary result vertex ids"))?;
    result_ids.extend(
        ctx.admit_iter(vertex_ids, "iges boundary result vertex traversal")?
            .flatten(),
    );
    Ok(BoundaryVertices {
        ids: result_ids,
        derivations,
        _storage: vertex_storage,
    })
}

fn point_position(
    index: &ModelIndex<'_>,
    id: &VertexId,
    ctx: &DecodeContext<'_>,
) -> Result<Option<FinitePoint3>, CodecError> {
    let Some(vertex) = index.vertices(id.as_str(), ctx)? else {
        return Ok(None);
    };
    Ok(index
        .points(vertex.point.as_str(), ctx)?
        .map(Point::position))
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
    index: &ModelIndex<'_>,
    support: &PcurveSupport<'_>,
    ctx: &DecodeContext<'_>,
) -> Result<ProceduralSourceParameterMap, CodecError> {
    let procedural = index.procedural_surface_for_surface(support.surface_id.as_str(), ctx)?;
    if let Some(procedural) = procedural.filter(|procedural| {
        matches!(
            procedural.definition(),
            ProceduralSurfaceDefinition::Extrusion(_) | ProceduralSurfaceDefinition::Revolution(_)
        )
    }) {
        return Ok(
            procedural_pcurve_parameter_map(index, procedural, ctx)?.map_or(
                ProceduralSourceParameterMap::Unavailable,
                ProceduralSourceParameterMap::Mapped,
            ),
        );
    }
    Ok(match support.geometry {
        SurfaceGeometry::Procedural { construction, .. } => {
            let mapped = match index.procedural_surfaces(construction.as_str(), ctx)? {
                Some(procedural) => procedural_pcurve_parameter_map(index, procedural, ctx)?,
                None => None,
            };
            mapped.map_or(
                ProceduralSourceParameterMap::Unavailable,
                ProceduralSourceParameterMap::Mapped,
            )
        }
        SurfaceGeometry::Solved(_) => ProceduralSourceParameterMap::NotApplicable,
    })
}

fn pcurve_parameter_map(
    support: &PcurveSupport<'_>,
    source: ProceduralSourceParameterMap,
) -> Option<(f64, f64, f64, f64)> {
    match source {
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
    index: Option<&DecodeModelIndex<'_, '_>>,
    sequence: u32,
    support: &PcurveSupport<'_>,
    tolerance: Option<f64>,
    ctx: &DecodeContext<'_>,
    composite_index: Option<&CompositeIndex>,
) -> Result<Option<(PcurveGeometry, [f64; 2])>, super::composite::CompositeCurveError> {
    let mut identity_storage = ctx.reserve_scoped(0, "iges pcurve source identity")?;
    let curve_id = identity_storage
        .with_storage(|| crate::ids::curve_admitted(&crate::ids::Stem::directory(sequence), ctx))?;
    let Some((nurbs, range)) =
        bounded_nurbs_for_curve_with_tolerance(ir, &curve_id, tolerance, ctx, composite_index)?
    else {
        return Ok(None);
    };
    let source_map = match index {
        Some(index) => procedural_source_parameter_map(index, support, ctx)?,
        None => match support.geometry {
            SurfaceGeometry::Solved(_) => ProceduralSourceParameterMap::NotApplicable,
            SurfaceGeometry::Procedural { .. } => ProceduralSourceParameterMap::Unavailable,
        },
    };
    let source_parameter_map = match source_map {
        ProceduralSourceParameterMap::Mapped(parameter_map) => Some(parameter_map),
        ProceduralSourceParameterMap::NotApplicable | ProceduralSourceParameterMap::Unavailable => {
            None
        }
    };
    let Some((u_factor, u_offset, v_factor, v_offset)) = pcurve_parameter_map(support, source_map)
    else {
        return Ok(None);
    };
    let map_point = |point: FinitePoint3| -> Result<FinitePoint2, NurbsError> {
        let point = point.get();
        let mapped = source_parameter_map.map_or_else(
            || {
                Point2::new(
                    point.x.mul_add(u_factor, u_offset),
                    point.y.mul_add(v_factor, v_offset),
                )
            },
            |(u_factor, u_offset, v_factor, v_offset)| {
                source_parameter_point_to_neutral(
                    Point2::new(point.x, point.y),
                    (u_factor, u_offset, v_factor, v_offset),
                    support.factor,
                )
            },
        );
        FinitePoint2::new(mapped).ok_or_else(|| {
            NurbsError::Structure("control_points contains a non-finite point".into())
        })
    };
    let (degree, knots, poles, periodic) = nurbs.into_parts();
    let poles = match poles {
        NurbsPoles3::Polynomial { points } => {
            let mut mapped =
                ctx.collection_vec(points.len(), "iges pcurve mapped polynomial poles")?;
            for point in ctx.admit_iter(points, "iges pcurve polynomial pole mapping")? {
                mapped.push(map_point(point)?);
            }
            PcurveNurbsPoles::Polynomial { points: mapped }
        }
        NurbsPoles3::Rational { points } => {
            let mut mapped =
                ctx.collection_vec(points.len(), "iges pcurve mapped rational poles")?;
            for pole in ctx.admit_iter(points, "iges pcurve rational pole mapping")? {
                mapped.push(WeightedPole2 {
                    point: map_point(pole.point)?,
                    weight: pole.weight,
                });
            }
            PcurveNurbsPoles::Rational { points: mapped }
        }
    };
    let parameter_curve = PcurveNurbs::new(ctx, degree, knots, poles, periodic)??;
    Ok(Some((
        PcurveGeometry::Nurbs {
            nurbs: parameter_curve,
        },
        range,
    )))
}

fn procedural_pcurve_parameter_map(
    index: &ModelIndex<'_>,
    procedural: &ProceduralSurface,
    ctx: &DecodeContext<'_>,
) -> Result<Option<(f64, f64, f64, f64)>, CodecError> {
    let Some([Some(carrier_start), Some(carrier_end), _, _]) =
        procedural.record_bounds().map(RecordBounds::get)
    else {
        return Ok(None);
    };
    let carrier_interval = [carrier_start, carrier_end];
    if carrier_interval[0] >= carrier_interval[1] {
        return Ok(None);
    }
    let directrix = match procedural.definition() {
        ProceduralSurfaceDefinition::Extrusion(payload) => Some(payload.directrix()),
        ProceduralSurfaceDefinition::Revolution(payload) => Some(payload.directrix()),
        _ => None,
    };
    let mut directrix_is_line = false;
    if let Some(directrix) = directrix {
        if let Some(curve) = index.curves(directrix.as_str(), ctx)? {
            let mut geometry = curve.geometry.solved();
            let mut steps = std::iter::repeat(());
            while let Some(current) = geometry {
                ctx.next_charged(&mut steps, "iges procedural directrix chain")?;
                match current {
                    SolvedCurveGeometry::Line(_) => {
                        directrix_is_line = true;
                        break;
                    }
                    SolvedCurveGeometry::Transformed(placed) => geometry = Some(placed.basis()),
                    _ => break,
                }
            }
        }
    }
    Ok((|| {
        let mut u_map = (1.0, 0.0);
        let mut v_map = (1.0, 0.0);
        match procedural.definition() {
            ProceduralSurfaceDefinition::Extrusion(definition_payload) => {
                let parameter_interval = definition_payload.parameter_interval();
                if directrix_is_line {
                    u_map = affine_parameter_map([0.0, 1.0], carrier_interval)?;
                } else if let Some(parameter_interval) = parameter_interval {
                    u_map = affine_parameter_map(parameter_interval.get(), carrier_interval)?;
                }
            }
            ProceduralSurfaceDefinition::Revolution(definition_payload) => {
                let angular_interval = definition_payload.angular_interval().endpoints();
                let angular_parameter_interval = definition_payload
                    .angular_parameter_interval()
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints);
                let parameter_interval = definition_payload
                    .parameter_interval()
                    .map(cadmpeg_ir::topology::IncreasingParameterInterval::endpoints);
                let transposed = definition_payload.transposed();
                let directrix_map = if directrix_is_line {
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
            _ => return None,
        }
        Some((u_map.0, u_map.1, v_map.0, v_map.1))
    })())
}

fn native_sequence_from_id(
    id: &str,
    prefix: &str,
    ctx: &DecodeContext<'_>,
) -> Result<Option<u32>, CodecError> {
    let Some(suffix) = id.strip_prefix(prefix) else {
        return Ok(None);
    };
    let end = ctx
        .position_by(
            suffix.bytes(),
            |byte| Ok(!byte.is_ascii_digit()),
            "iges native identity digits",
        )?
        .unwrap_or(suffix.len());
    if end == 0 {
        return Ok(None);
    }
    Ok(ctx
        .parse_text::<u32>(&suffix[..end], "iges native identity sequence")?
        .ok())
}

fn parameter_curve_carrier_id(
    sequence: u32,
    entries: &[DirectoryEntry],
    records: &[ParameterRecord],
    ctx: &DecodeContext<'_>,
) -> Result<Option<CurveId>, CodecError> {
    let Some(entry) = entry_by_sequence(entries, sequence, ctx)? else {
        return Ok(None);
    };
    let carrier_sequence = if entry.entity_type == 142 && entry.form == 0 {
        let Some(carrier_sequence) = record_by_sequence(records, sequence, ctx)?
            .and_then(|record| record.integer(3))
            .and_then(|value| u32::try_from(value).ok())
            .filter(|sequence| sequence % 2 == 1)
        else {
            return Ok(None);
        };
        carrier_sequence
    } else {
        sequence
    };
    Ok(Some(crate::ids::curve_admitted(
        &crate::ids::Stem::directory(carrier_sequence),
        ctx,
    )?))
}

fn surface_parameter_bound_intervals(
    bounds: Option<[Option<f64>; 4]>,
    surface_id: &SurfaceId,
    entries: &[DirectoryEntry],
    records: &[ParameterRecord],
    precision: RealPrecision,
    ctx: &DecodeContext<'_>,
) -> Result<Option<[Option<DeclaredInterval>; 4]>, CodecError> {
    let Some(bounds) = bounds else {
        return Ok(None);
    };
    let mut intervals = bounds.map(|bound| bound.map(|value| DeclaredInterval::around(value, 0.0)));
    let Some(sequence) = native_sequence_from_id(surface_id.as_str(), "iges:model:surface#D", ctx)?
    else {
        return Ok(Some(intervals));
    };
    let Some(entry) = entry_by_sequence(entries, sequence, ctx)? else {
        return Ok(Some(intervals));
    };
    if entry.entity_type != 128 {
        return Ok(Some(intervals));
    }
    let Some(record) = record_by_sequence(records, sequence, ctx)? else {
        return Ok(Some(intervals));
    };
    if let Some(declared) = super::surfaces::type128_parameter_bound_intervals(record, precision) {
        for (bound, declared) in intervals.iter_mut().zip(declared) {
            if bound.is_some() {
                *bound = Some(declared);
            }
        }
    }
    Ok(Some(intervals))
}

fn source_curve_control_intervals(
    index: &ModelIndex<'_>,
    curve_id: &CurveId,
    tables: (&[DirectoryEntry], &[ParameterRecord]),
    precision: RealPrecision,
    factor: f64,
    active: &mut BTreeSet<CurveId>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<[DeclaredInterval; 3]>>, CodecError> {
    let (entries, records) = tables;
    let _nested = ctx.enter_nested("iges source curve intervals")?;
    if ctx.contains_btree_set(active, curve_id, "iges source active curve lookup")? {
        return Ok(None);
    }
    let active_id = curve_id.try_clone_for_decode(ctx, "iges source active curve ID")?;
    ctx.insert_btree_set(active, active_id, "iges source active curve nodes")?;
    let result = (|| -> Result<Option<Vec<[DeclaredInterval; 3]>>, CodecError> {
        let Some(curve) = index.curves(curve_id.as_str(), ctx)? else {
            return Ok(None);
        };
        let native = match native_sequence_from_id(curve_id.as_str(), "iges:model:curve#D", ctx)? {
            Some(sequence) => {
                let Some(entry) = entry_by_sequence(entries, sequence, ctx)? else {
                    return Ok(None);
                };
                Some((sequence, entry))
            }
            None => None,
        };
        if let Some((sequence, entry)) = native {
            if entry.entity_type == 102 && entry.form == 0 {
                let Some(record) = record_by_sequence(records, sequence, ctx)? else {
                    return Ok(None);
                };
                let Some(child_count) = record.count(1) else {
                    return Ok(None);
                };
                let mut child_ids =
                    ctx.collection_vec(child_count, "iges source composite child IDs")?;
                for offset in ctx.admit_iter(0..child_count, "iges source composite children")? {
                    let Some(child_sequence) = offset
                        .checked_add(2)
                        .and_then(|index| record.integer(index))
                        .and_then(|value| u32::try_from(value).ok())
                    else {
                        return Ok(None);
                    };
                    let Some(child_id) =
                        parameter_curve_carrier_id(child_sequence, entries, records, ctx)?
                    else {
                        return Ok(None);
                    };
                    child_ids.push(child_id);
                }
                let mut controls = Vec::new();
                for child_id in ctx.admit_iter(child_ids, "iges source child control traversal")? {
                    let Some(child) = source_curve_control_intervals(
                        index, &child_id, tables, precision, factor, active, ctx,
                    )?
                    else {
                        return Ok(None);
                    };
                    ctx.extend_vec(&mut controls, child, "iges source composite controls")?;
                }
                return Ok((!controls.is_empty()).then_some(controls));
            }
        }
        match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Composite { segments, .. }) => {
                let mut controls = Vec::new();
                for segment in
                    ctx.admit_iter(&segments[..], "iges source solved composite traversal")?
                {
                    let Some(child) = source_curve_control_intervals(
                        index,
                        &segment.curve,
                        tables,
                        precision,
                        factor,
                        active,
                        ctx,
                    )?
                    else {
                        return Ok(None);
                    };
                    ctx.extend_vec(
                        &mut controls,
                        child,
                        "iges source solved composite controls",
                    )?;
                }
                Ok((!controls.is_empty()).then_some(controls))
            }
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => {
                if matches!(nurbs.pole_rows(), NurbsPoles3::Rational { .. })
                    && ctx.any_by(
                        0..nurbs.pole_count(),
                        |index| {
                            Ok(nurbs
                                .pole_rows()
                                .weight_at(index)
                                .is_some_and(|weight| weight <= 0.0))
                        },
                        "iges source positive weights",
                    )?
                {
                    return Ok(None);
                }
                let exact = || -> Result<Vec<[DeclaredInterval; 3]>, CodecError> {
                    let mut controls =
                        ctx.collection_vec(nurbs.pole_count(), "iges source exact controls")?;
                    for index in ctx
                        .admit_iter(0..nurbs.pole_count(), "iges source exact control traversal")?
                    {
                        let point = nurbs
                            .pole_rows()
                            .point_at(index)
                            .ok_or_else(|| CodecError::malformed("source curve pole is missing"))?
                            .get();
                        controls.push(
                            [point.x, point.y, point.z]
                                .map(|value| DeclaredInterval::around(value, 0.0)),
                        );
                    }
                    Ok(controls)
                };
                let Some((sequence, entry)) = native else {
                    return Ok(Some(exact()?));
                };
                if entry.entity_type != 126 {
                    return Ok(Some(exact()?));
                }
                if entry.transform != 0 {
                    return Ok(None);
                }
                let Some(record) = record_by_sequence(records, sequence, ctx)? else {
                    return Ok(None);
                };
                let Some(mut raw_controls) =
                    super::geometry::type126_declared_control_points(record, precision, ctx)?
                else {
                    return Ok(None);
                };
                if raw_controls.len() != nurbs.pole_count() {
                    return Ok(None);
                }
                for control in
                    ctx.admit_iter(&mut raw_controls, "iges source control interval scaling")?
                {
                    *control = control.map(|value| value.scale(factor));
                }
                Ok(Some(raw_controls))
            }
            _ => Ok(None),
        }
    })();
    ctx.remove_btree_set(active, curve_id, "iges source active curve removal")?;
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
    index: &ModelIndex<'_>,
    curve_id: &CurveId,
    support: &PcurveSupport<'_>,
    bounds: Option<[Option<DeclaredInterval>; 4]>,
    tables: (&[DirectoryEntry], &[ParameterRecord]),
    precision: RealPrecision,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut scratch =
        ctx.reserve_scoped(0, "iges source curve control polygon within bounds scratch")?;
    scratch.with_storage(|| {
        let Some(bounds) = bounds else {
            return Ok(true);
        };
        let source_map = procedural_source_parameter_map(index, support, ctx)?;
        let Some((u_factor, u_offset, v_factor, v_offset)) =
            pcurve_parameter_map(support, source_map)
        else {
            return Ok(false);
        };
        let Some(controls) = source_curve_control_intervals(
            index,
            curve_id,
            tables,
            precision,
            support.factor,
            &mut BTreeSet::new(),
            ctx,
        )?
        else {
            return Ok(false);
        };
        Ok(!controls.is_empty()
            && ctx.all_by(
                controls,
                |[u, v, _]| {
                    let Some(u) = affine_parameter_interval(u, u_factor, u_offset) else {
                        return Ok(false);
                    };
                    let Some(v) = affine_parameter_interval(v, v_factor, v_offset) else {
                        return Ok(false);
                    };
                    Ok(parameter_interval_reaches_bounds(u, bounds[0], bounds[1])
                        && parameter_interval_reaches_bounds(v, bounds[2], bounds[3]))
                },
                "iges source control bound proof",
            )?)
    })
}

fn linear_model_nurbs_points(
    nurbs: &NurbsCurve,
    range: [f64; 2],
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<Point3>>, CodecError> {
    if matches!(nurbs.pole_rows(), NurbsPoles3::Rational { .. })
        && ctx.any_by(
            0..nurbs.pole_count(),
            |index| {
                Ok(nurbs
                    .pole_rows()
                    .weight_at(index)
                    .is_some_and(|weight| weight != 1.0))
            },
            "iges linear boundary weights",
        )?
    {
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
    let mut points = Vec::new();
    let mut parameters = parameters;
    while let Some(parameter) =
        ctx.next_charged(&mut parameters, "iges linear boundary parameter traversal")?
    {
        let Some(point) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::nurbs_curve_point_at(ctx, nurbs, parameter),
        )?)?
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut points,
            point.get(),
            "iges linear model boundary points",
        )?;
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
    if matches!(nurbs.pole_rows(), PcurveNurbsPoles::Rational { .. })
        && ctx.any_by(
            0..nurbs.pole_rows().count(),
            |index| {
                Ok(nurbs
                    .pole_rows()
                    .weight_at(index)
                    .is_some_and(|weight| weight != 1.0))
            },
            "iges linear boundary weights",
        )?
    {
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
    let mut points = Vec::new();
    let mut parameters = parameters;
    while let Some(parameter) =
        ctx.next_charged(&mut parameters, "iges linear boundary parameter traversal")?
    {
        let Some(point) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::pcurve_uv(ctx, geometry, parameter),
        )?)?
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut points,
            [point.u, point.v],
            "iges linear parameter boundary points",
        )?;
    }
    Ok(Some(points))
}

fn append_path<T: Copy + PartialEq>(
    target: &mut Vec<T>,
    path: &[T],
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
    let first = path.len() - additional;
    ctx.extend_from_slice(target, &path[first..], "iges linear boundary path")?;
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
    for item in ctx.admit_iter(items, "iges linear boundary items")? {
        let Some(curve) = index.curves(item.model_curve.as_str(), ctx)? else {
            return Ok(None);
        };
        let mut curve_points = match curve.geometry.solved() {
            Some(SolvedCurveGeometry::Line(_)) => {
                let mut line = ctx.collection_vec(2, "iges linear model boundary line")?;
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
            ctx.reverse(&mut curve_points, "iges model ring reversal")?;
        }
        if !append_path(&mut points, &curve_points, ctx)? {
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
    for item in ctx.admit_iter(items, "iges linear boundary items")? {
        let Some(curve) = index.curves(item.model_curve.as_str(), ctx)? else {
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
            ctx,
        )? {
            return Ok(None);
        }
    }
    let Some(model_coordinates) = plane_coordinates(&model_points, model_plane, ctx)? else {
        return Ok(None);
    };
    if surface_kind == BoundarySurfaceKind::Trimmed
        && ctx.any_by(
            items,
            |item| Ok(item.segment.parameter_curves_authoritative),
            "iges authoritative boundary search",
        )?
    {
        if !ctx.all_by(
            items,
            |item| Ok(item.segment.parameter_curves_authoritative),
            "iges authoritative boundary proof",
        )? {
            return Ok(None);
        }
        let mut parameter_points = Vec::new();
        for item in ctx.admit_iter(items, "iges linear boundary items")? {
            if item.pcurves.is_empty() {
                return Ok(None);
            }
            for (geometry, range) in
                ctx.admit_iter(&item.pcurves, "iges linear boundary pcurves")?
            {
                let Some(points) = linear_pcurve_points(geometry, *range, ctx)? else {
                    return Ok(None);
                };
                if !append_path(&mut parameter_points, &points, ctx)? {
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
    fn new(
        points: Vec<[f64; 2]>,
        ctx: &DecodeContext<'_>,
    ) -> Result<Result<Self, NonSimpleRing>, CodecError> {
        if points.len() < 4 || points.first() != points.last() {
            return Ok(Err(NonSimpleRing));
        }
        if ctx.any_by(
            &points,
            |point| Ok(point.iter().any(|coordinate| !coordinate.is_finite())),
            "iges simple ring coordinate admission",
        )? || ctx.any_by(
            points.windows(2),
            |segment| Ok(segment[0] == segment[1]),
            "iges simple ring adjacent vertex comparisons",
        )? {
            return Ok(Err(NonSimpleRing));
        }
        if super::geometry::closed_polyline_has_duplicate(
            &points,
            |left, right| left == right,
            ctx,
        )? {
            return Ok(Err(NonSimpleRing));
        }
        if planar_polyline_has_self_intersection(&points, ctx)? {
            return Ok(Err(NonSimpleRing));
        }
        Ok(Ok(Self(points)))
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

fn planar_point_is_strictly_inside(
    point: [f64; 2],
    ring: &SimpleRing,
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if ctx.any_by(
        ring.points().windows(2),
        |segment| {
            Ok(super::geometry::planar_segments_contain_point(
                point,
                [segment[0], segment[1]],
            ))
        },
        "iges planar point boundary comparisons",
    )? {
        return Ok(false);
    }
    let mut inside = false;
    for index in ctx.admit_iter(
        0..ring.points().len() - 1,
        "iges planar point containment comparisons",
    )? {
        let segment = &ring.points()[index..=index + 1];
        let [left, right] = [segment[0], segment[1]];
        if (left[1] > point[1]) != (right[1] > point[1]) {
            let crossing =
                left[0] + (right[0] - left[0]) * (point[1] - left[1]) / (right[1] - left[1]);
            if point[0] < crossing {
                inside = !inside;
            }
        }
    }
    Ok(inside)
}

fn linear_boundary_rings(
    candidates: &[Option<LinearBoundaryGeometry>],
    space: BoundarySpace,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Result<Vec<SimpleRing>, NonSimpleRing>>, CodecError> {
    if candidates.is_empty() {
        return Ok(None);
    }
    let mut rings = ctx.collection_vec(candidates.len(), "iges linear boundary ring slots")?;
    for candidate in ctx.admit_iter(candidates, "iges linear boundary candidates")? {
        let ((BoundarySpace::Parameter, Some(LinearBoundaryGeometry::Parameter(points)))
        | (BoundarySpace::Model, Some(LinearBoundaryGeometry::Model(points)))) =
            (space, candidate.as_ref())
        else {
            return Ok(None);
        };
        let copied = ctx.copy_slice(points, "iges linear boundary ring points")?;
        match SimpleRing::new(copied, ctx)? {
            Ok(ring) => rings.push(ring),
            Err(error) => return Ok(Some(Err(error))),
        }
    }
    Ok(Some(Ok(rings)))
}

fn rings_are_disjoint(rings: &[SimpleRing], ctx: &DecodeContext<'_>) -> Result<bool, CodecError> {
    Ok(!ctx.any_by(
        rings.iter().enumerate(),
        |(left_index, left)| {
            ctx.any_by(
                &rings[left_index + 1..],
                |right| {
                    Ok(
                        planar_polylines_intersect(left.points(), right.points(), ctx)?
                            || planar_point_is_strictly_inside(left.first(), right, ctx)?
                            || planar_point_is_strictly_inside(right.first(), left, ctx)?,
                    )
                },
                "iges inner ring pair comparisons",
            )
        },
        "iges inner ring traversal",
    )?)
}

fn inner_boundaries_are_disjoint_and_inside(
    outer: &SimpleRing,
    inners: &[SimpleRing],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    if ctx.any_by(
        inners,
        |inner| {
            Ok(
                planar_polylines_intersect(outer.points(), inner.points(), ctx)?
                    || !ctx.all_by(
                        inner.interior(),
                        |point| planar_point_is_strictly_inside(*point, outer, ctx),
                        "iges inner ring containment traversal",
                    )?,
            )
        },
        "iges inner ring containment proof",
    )? {
        return Ok(false);
    }
    rings_are_disjoint(inners, ctx)
}

fn linear_boundary_relationship_is_valid(
    rings: Result<&[SimpleRing], &NonSimpleRing>,
    surface_kind: BoundarySurfaceKind,
    has_explicit_outer: bool,
    support: &SurfaceGeometry,
    support_bounds: Option<[Option<f64>; 4]>,
    periodic_parameters: [bool; 2],
    ctx: &DecodeContext<'_>,
) -> Result<Option<bool>, CodecError> {
    let rings = match rings {
        Ok(rings) => rings,
        Err(NonSimpleRing) => return Ok(Some(false)),
    };
    if surface_kind == BoundarySurfaceKind::Bounded {
        return Ok(Some(true));
    }
    if has_explicit_outer {
        let Some((outer, inners)) = rings.split_first() else {
            return Ok(None);
        };
        return Ok(Some(inner_boundaries_are_disjoint_and_inside(
            outer, inners, ctx,
        )?));
    }
    if periodic_parameters.iter().any(|periodic| *periodic) {
        return Ok(None);
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
            if ctx.any_by(
                rings,
                |ring| {
                    ctx.any_by(
                        ring.interior(),
                        |point| {
                            Ok(point[0] <= u_lower
                                || point[0] >= u_upper
                                || point[1] <= v_lower
                                || point[1] >= v_upper)
                        },
                        "iges ring support bound comparisons",
                    )
                },
                "iges ring support bound traversal",
            )? {
                return Ok(Some(false));
            }
        }
        Some(_) => return Ok(None),
        None if !matches!(
            support,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
        ) =>
        {
            return Ok(None);
        }
        None => {}
    }
    Ok(Some(rings_are_disjoint(rings, ctx)?))
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
    insertion: (f64, usize, usize),
    ctx: &DecodeContext<'_>,
) -> Result<Option<()>, CodecError> {
    let (knot, span, multiplicity) = insertion;
    let Some((left_end, tail_start)) = span.checked_sub(degree).zip(span.checked_sub(multiplicity))
    else {
        return Ok(None);
    };
    ctx.insert_vec(
        controls,
        tail_start + 1,
        controls[tail_start],
        "iges pcurve inserted controls",
    )?;
    for index in ctx
        .admit_iter(left_end + 1..=tail_start, "iges pcurve knot interpolation")?
        .rev()
    {
        let denominator = knots[index + degree] - knots[index];
        if !denominator.is_finite() || denominator <= 0.0 {
            return Ok(None);
        }
        let alpha = (knot - knots[index]) / denominator;
        controls[index] = std::array::from_fn(|axis| {
            alpha * controls[index][axis] + (1.0 - alpha) * controls[index - 1][axis]
        });
    }
    ctx.insert_vec(knots, span + 1, knot, "iges pcurve inserted knots")?;
    Ok(Some(()))
}

fn homogeneous_pcurve_spans(
    degree: usize,
    knots: &[f64],
    mut controls: Vec<[f64; 4]>,
    ctx: &DecodeContext<'_>,
) -> Result<Option<Vec<HomogeneousPcurveSpan>>, CodecError> {
    let Some(expected_knots) = controls
        .len()
        .checked_add(degree)
        .and_then(|value| value.checked_add(1))
    else {
        return Ok(None);
    };
    if degree == 0
        || degree >= controls.len()
        || knots.len() != expected_knots
        || ctx.any_by(
            knots,
            |knot| Ok(!knot.is_finite()),
            "iges pcurve finite knots",
        )?
        || ctx.any_by(
            knots.windows(2),
            |pair| Ok(pair[0] > pair[1]),
            "iges pcurve knot order",
        )?
    {
        return Ok(None);
    }
    let Some(domain) = knots
        .get(degree)
        .copied()
        .zip(knots.get(controls.len()).copied())
    else {
        return Ok(None);
    };
    let domain = [domain.0, domain.1];
    if domain[0] >= domain[1] {
        return Ok(None);
    }
    let (mut copied_knots, mut knot_storage) =
        ctx.copy_temporary_slice(knots, "iges pcurve knot copy")?;
    let mut previous = knots[0];
    let mut multiplicity = 0;
    let mut inserted = 0;
    for (index, &knot) in ctx.admit_iter(knots, "iges pcurve knot runs")?.enumerate() {
        if knot == previous {
            if knot.total_cmp(&previous).is_lt() {
                previous = knot;
            }
            multiplicity += 1;
            continue;
        }
        if domain[0] < previous && previous < domain[1] {
            let span = index - 1 + inserted;
            for (offset, count) in ctx
                .admit_iter(multiplicity..degree, "iges pcurve knot insertions")?
                .enumerate()
            {
                if knot_storage
                    .with_storage(|| {
                        insert_homogeneous_pcurve_knot(
                            degree,
                            &mut copied_knots,
                            &mut controls,
                            (previous, span + offset, count),
                            ctx,
                        )
                    })?
                    .is_none()
                {
                    return Ok(None);
                }
                inserted += 1;
            }
        }
        previous = knot;
        multiplicity = 1;
    }
    let mut spans = Vec::new();
    for span in ctx.admit_iter(degree..controls.len(), "iges pcurve span traversal")? {
        let Some((start, end)) = copied_knots
            .get(span)
            .copied()
            .zip(copied_knots.get(span + 1).copied())
        else {
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
        let copied_controls = ctx.copy_slice(span_controls, "iges pcurve span controls")?;
        ctx.reserve_vec(&mut spans, 1, "iges pcurve span descriptors")?;
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
    let (mut current, _working_storage) =
        ctx.copy_temporary_slice(controls, "iges pcurve split first controls")?;
    let mut left = ctx.collection_vec(controls.len(), "iges pcurve split left controls")?;
    let mut right = ctx.collection_vec(controls.len(), "iges pcurve split right controls")?;
    left.push(current[0]);
    right.push(current[current.len() - 1]);
    for _ in ctx.admit_iter(1..controls.len(), "iges pcurve split traversal")? {
        for index in ctx.admit_iter(0..current.len() - 1, "iges pcurve split interpolation")? {
            let pair = [current[index], current[index + 1]];
            current[index] = std::array::from_fn(|axis| {
                (1.0 - parameter) * pair[0][axis] + parameter * pair[1][axis]
            });
        }
        current.pop();
        left.push(current[0]);
        right.push(current[current.len() - 1]);
    }
    ctx.reverse(&mut right, "iges pcurve split right reversal")?;
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
        ctx.reverse(&mut restricted, "iges restricted pcurve reversal")?;
        return Ok(Some(restricted));
    }
    if start == end {
        let Some(point) = split_homogeneous_pcurve(controls, start, ctx)?
            .and_then(|(left, _)| left.last().copied())
        else {
            return Ok(None);
        };
        let mut result = ctx.collection_vec(1, "iges pcurve restricted point")?;
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
    let (ctx, _) =
        DecodeContext::from_root_bytes(&[], &arena, &cadmpeg_core::decode::DecodePolicy::service())
            .expect("test setup");
    let bounds = bounds
        .map(|bounds| bounds.map(|bound| bound.map(|value| DeclaredInterval::around(value, 0.0))));
    pcurve_within_declared_intervals(geometry, range, bounds, periodic, &ctx).expect("test setup")
}

fn pcurve_within_declared_intervals(
    geometry: &PcurveGeometry,
    range: [f64; 2],
    bounds: Option<[Option<DeclaredInterval>; 4]>,
    periodic: [bool; 2],
    ctx: &DecodeContext<'_>,
) -> Result<bool, CodecError> {
    let mut scratch = ctx.reserve_scoped(0, "iges pcurve within declared intervals scratch")?;
    scratch.with_storage(|| {
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
        let (mut controls, control_storage) = ctx.temporary_vec(
            nurbs.pole_rows().count(),
            "iges pcurve homogeneous controls",
        )?;
        for index in ctx.admit_iter(
            0..nurbs.pole_rows().count(),
            "iges homogeneous control traversal",
        )? {
            let Some(point) = nurbs
                .pole_rows()
                .point_at(index)
                .map(cadmpeg_ir::units::FinitePoint2::get)
            else {
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
        drop(control_storage);
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
        let expand_periodic =
            |lower: Option<DeclaredInterval>, upper: Option<DeclaredInterval>, periodic: bool| {
                match (lower, upper, periodic) {
                    (Some(lower), Some(upper), true) => {
                        let lower = DeclaredInterval::around(lower.lower_bound(), 0.0);
                        let upper = DeclaredInterval::around(upper.upper_bound(), 0.0);
                        let period = upper.subtract(lower);
                        (period.is_finite() && period.is_strictly_positive())
                            .then_some((Some(lower.subtract(period)), Some(upper.add(period))))
                    }
                    _ => Some((lower, upper)),
                }
            };
        let Some((u_lower, u_upper)) = expand_periodic(bounds[0], bounds[1], periodic[0]) else {
            return Ok(false);
        };
        let Some((v_lower, v_upper)) = expand_periodic(bounds[2], bounds[3], periodic[1]) else {
            return Ok(false);
        };
        let mut covered = false;
        for span in ctx.admit_iter(spans, "iges pcurve bounded span traversal")? {
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
            let mut restricted_storage = ctx.reserve_scoped(0, "iges restricted pcurve scratch")?;
            let Some(restricted) = restricted_storage.with_storage(|| {
                restrict_homogeneous_pcurve(&span.controls, local_start.get(), local_end.get(), ctx)
            })?
            else {
                return Ok(false);
            };
            if ctx.any_by(
                &restricted,
                |control| {
                    let weight = control[0];
                    Ok(!weight.is_finite()
                        || weight <= 0.0
                        || !in_bound(control[1] / weight, u_lower, u_upper)
                        || !in_bound(control[2] / weight, v_lower, v_upper))
                },
                "iges restricted pcurve bounds",
            )? {
                return Ok(false);
            }
        }
        Ok(covered)
    })
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
    let mut scratch = ctx.reserve_scoped(0, "iges surface parameter bounds scratch")?;
    scratch.with_storage(|| {
        fn visit(
            index: &ModelIndex<'_>,
            surface_id: &SurfaceId,
            visiting: &mut BTreeSet<SurfaceId>,
            ctx: &DecodeContext<'_>,
        ) -> Result<Option<[Option<f64>; 4]>, CodecError> {
            let _nested = ctx.enter_nested("iges support-bound surface chain")?;
            if ctx.contains_btree_set(visiting, surface_id, "iges support-bound visited lookup")? {
                return Ok(None);
            }
            let visited_id =
                surface_id.try_clone_for_decode(ctx, "iges support-bound visiting surface ID")?;
            ctx.insert_btree_set(
                visiting,
                visited_id,
                "iges support-bound visiting surface nodes",
            )?;
            let Some(procedural) =
                index.procedural_surface_for_surface(surface_id.as_str(), ctx)?
            else {
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
    })
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
    let mut previous_end = None;
    let agrees = ctx.all_by(
        pcurves,
        |(geometry, range)| -> Result<bool, CodecError> {
            let Some(start_uv) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::pcurve_uv(ctx, geometry, range[0]),
            )?)?
            else {
                return Ok(false);
            };
            let Some(start) = finite_or_refusal(cadmpeg_ir::eval::model_surface_point_by_id(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
                index,
                surface_id,
                start_uv.u,
                start_uv.v,
            ))?
            else {
                return Ok(false);
            };
            if !close(
                start.get(),
                previous_end.unwrap_or(expected_start),
                tolerance,
            ) {
                return Ok(false);
            }
            let Some(end_uv) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::pcurve_uv(ctx, geometry, range[1]),
            )?)?
            else {
                return Ok(false);
            };
            let Some(end) = finite_or_refusal(cadmpeg_ir::eval::model_surface_point_by_id(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(ctx),
                index,
                surface_id,
                end_uv.u,
                end_uv.v,
            ))?
            else {
                return Ok(false);
            };
            previous_end = Some(end.get());
            Ok(true)
        },
        "iges trimmed pcurve endpoint traversal",
    )?;
    Ok(agrees && previous_end.is_some_and(|end| close(end, expected_end, tolerance)))
}

fn edge_range_matches_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    edge: &Edge,
    carrier_index: &ModelIndex<'_>,
    start: Point3,
    end: Point3,
    tolerance: f64,
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let Some(curve_id) = edge.curve() else {
        return Ok(false);
    };
    let Some(curve) = carrier_index.curves(curve_id.as_str(), ctx)? else {
        return Ok(false);
    };
    let Some(range) = edge.param_range() else {
        return Ok(false);
    };
    if !range.iter().all(|parameter| parameter.is_finite()) {
        return Ok(false);
    }
    let geometry = &curve.geometry;
    let Some(evaluated_start) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
        cadmpeg_ir::eval::decode::curve_point(ctx, geometry, range[0]),
    )?)?
    else {
        return Ok(false);
    };
    let Some(evaluated_end) = finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
        cadmpeg_ir::eval::decode::curve_point(ctx, geometry, range[1]),
    )?)?
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
    let mut endpoints_found = false;
    let mut matched = None;
    let mut multiple_matched = false;
    let mut agreeing = None;
    let ambiguous = ctx.any_by(
        candidates,
        |&edge| {
            let Some(start) = point_position(carrier_index, &edge.start, ctx)? else {
                return Ok(false);
            };
            let Some(end) = point_position(carrier_index, &edge.end, ctx)? else {
                return Ok(false);
            };
            endpoints_found = true;
            if !edge_range_matches_curve(
                ctx,
                edge,
                carrier_index,
                start.get(),
                end.get(),
                tolerance,
            )? {
                return Ok(false);
            }
            if matched.is_some() {
                multiple_matched = true;
            } else {
                matched = Some((edge, start, end));
            }
            if pcurves.is_empty() {
                return Ok(multiple_matched);
            }
            let (expected_start, expected_end) = if sense == Sense::Forward {
                (start, end)
            } else {
                (end, start)
            };
            if pcurves_agree(
                carrier_index,
                surface_id,
                pcurves,
                expected_start.get(),
                expected_end.get(),
                tolerance,
                ctx,
            )? {
                if agreeing.is_some() {
                    return Ok(true);
                }
                agreeing = Some((edge, start, end));
            }
            Ok(false)
        },
        "iges trimmed edge candidate traversal",
    )?;
    if ambiguous {
        return Err(BoundaryEdgeSelectionError::Ambiguous);
    }
    let Some(matched) = matched else {
        return Err(if endpoints_found {
            BoundaryEdgeSelectionError::InvalidRange
        } else {
            BoundaryEdgeSelectionError::MissingEndpoints
        });
    };
    let (selected, pcurves_agree) = if pcurves.is_empty() {
        (matched, true)
    } else if let Some(agreeing) = agreeing {
        (agreeing, true)
    } else if !parameter_curves_authoritative && !multiple_matched {
        (matched, false)
    } else {
        return Err(if parameter_curves_authoritative {
            BoundaryEdgeSelectionError::PcurveDisagreement
        } else {
            BoundaryEdgeSelectionError::Ambiguous
        });
    };
    let (edge, start, end) = selected;
    Ok((clone_boundary_edge(edge, ctx)?, start, end, pcurves_agree))
}

pub(super) fn clone_boundary_edge(
    edge: &Edge,
    ctx: &DecodeContext<'_>,
) -> Result<Edge, CodecError> {
    let carrier = match &edge.carrier {
        cadmpeg_ir::topology::EdgeCarrier::Free => cadmpeg_ir::topology::EdgeCarrier::Free,
        cadmpeg_ir::topology::EdgeCarrier::Endpoints(range) => {
            cadmpeg_ir::topology::EdgeCarrier::Endpoints(*range)
        }
        cadmpeg_ir::topology::EdgeCarrier::Curve(curve) => {
            cadmpeg_ir::topology::EdgeCarrier::Curve(
                curve.try_clone_for_decode(ctx, "iges selected edge curve ID")?,
            )
        }
        cadmpeg_ir::topology::EdgeCarrier::Bounded(curve, range) => {
            cadmpeg_ir::topology::EdgeCarrier::Bounded(
                curve.try_clone_for_decode(ctx, "iges selected edge curve ID")?,
                *range,
            )
        }
    };
    Ok(Edge {
        id: edge.id.try_clone_for_decode(ctx, "iges selected edge ID")?,
        carrier,
        start: edge
            .start
            .try_clone_for_decode(ctx, "iges selected edge start ID")?,
        end: edge
            .end
            .try_clone_for_decode(ctx, "iges selected edge end ID")?,
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
    let mut lookup_storage = ctx.reserve_scoped(0, "IGES projection source lookup")?;
    let records = parameters;
    let entries = directory;
    let mut decoded = BTreeSet::new();
    let mut losses = Vec::new();
    let mut boundary_vertex_derivations = Vec::new();
    let mut boundaries = BTreeMap::new();

    let carrier_index = ModelIndex::new_model_only(ir, ctx)?;
    let mut composite_storage = ctx.reserve_scoped(0, "IGES trimming composite index")?;
    let mut composite_index: Option<CompositeIndex> = None;
    let mut edges_by_curve = BTreeMap::<&CurveId, Vec<&Edge>>::new();
    for edge in ctx.admit_iter(&ir.model.edges, "iges boundary carrier traversal")? {
        if let Some(curve) = edge.curve() {
            lookup_storage.with_storage(|| {
                ctx.admit_btree_entry(&edges_by_curve, &curve, "iges boundary carrier index nodes")
            })?;
            let group = edges_by_curve.entry(curve).or_default();
            lookup_storage.with_storage(|| {
                ctx.reserve_vec(group, 1, "iges boundary carrier edge references")
            })?;
            group.push(edge);
        }
    }
    let mut staged = Vec::new();
    let mut staged_storage = ctx.reserve_scoped(0, "iges trimming staged candidates")?;
    for entry in ctx
        .admit_iter(directory, "iges trimming directory traversal")?
        .filter(|entry| entry.entity_type == 142 && entry.form == 0)
    {
        let Some(record) = record_by_sequence(records, entry.sequence, ctx)? else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(preference) = record
            .integer(5)
            .filter(|value| matches!(value, 0..=3) && matches!(record.integer(1), Some(0..=3)))
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    "curve-on-surface creation or preference flag is invalid"
                ),
            )?;
            continue;
        };
        let Some(surface) = pointer(record, 2) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "curve-on-surface surface pointer is invalid"),
            )?;
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
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "curve-on-surface parameter curve pointer is invalid"),
            )?;
            continue;
        }
        let Some(model_curve) = pointer(record, 4) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "curve-on-surface model curve pointer is invalid"),
            )?;
            continue;
        };
        if match pcurve {
            Some(pcurve) => entry_by_sequence(entries, pcurve, ctx)?.is_none_or(|entry| {
                entry.status.use_flag(global.global_table()) != Some(UseFlag::Parametric)
            }),
            None => false,
        } {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "parameter curve does not have entity-use flag 05"),
            )?;
            continue;
        }
        let mut pcurves = lookup_storage.with_storage(|| {
            ctx.collection_vec(
                usize::from(pcurve.is_some()),
                "iges Type142 boundary pcurve pointers",
            )
        })?;
        if let Some(pcurve) = pcurve {
            pcurves.push(pcurve);
        }
        let mut segments = lookup_storage
            .with_storage(|| ctx.collection_vec(1, "iges Type142 boundary segments"))?;
        segments.push(BoundarySegment {
            pcurves,
            model_curve,
            sense: Sense::Forward,
            parameter_curves_authoritative: pcurve.is_some() && preference != 2,
        });
        lookup_storage.with_storage(|| {
            ctx.insert_btree_map(
                &mut boundaries,
                entry.sequence,
                BoundaryDefinition { surface, segments },
                "iges trimming boundary index nodes",
            )
        })?;
        ctx.insert_btree_set(
            &mut decoded,
            entry.sequence,
            "iges trimming decoded sequences",
        )?;
    }
    for entry in ctx
        .admit_iter(directory, "iges trimming directory traversal")?
        .filter(|entry| entry.entity_type == 141 && entry.form == 0)
    {
        let Some(record) = record_by_sequence(records, entry.sequence, ctx)? else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
            continue;
        };
        let Some(boundary_type) = record.integer(1).filter(|value| matches!(value, 0 | 1)) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "boundary representation type is not 0 or 1"),
            )?;
            continue;
        };
        let Some(preference) = record.integer(2).filter(|value| matches!(value, 0..=3)) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "boundary preference flag is invalid"),
            )?;
            continue;
        };
        let Some(surface) = pointer(record, 3) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "boundary support pointer is invalid"),
            )?;
            continue;
        };
        let Some(segment_count) = record.count(4).filter(|count| *count > 0) else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "boundary segment count is not positive"),
            )?;
            continue;
        };
        let mut index = 5;
        let mut segments = lookup_storage
            .with_storage(|| ctx.collection_vec(segment_count, "iges Type141 boundary segments"))?;
        let mut valid = true;
        for _ in ctx.admit_iter(0..segment_count, "iges Type141 segment traversal")? {
            let Some(model_curve) = pointer(record, index) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "boundary model-curve pointer is invalid"),
                )?;
                valid = false;
                break;
            };
            let sense = match record.integer(index + 1) {
                Some(1) => Sense::Forward,
                Some(2) => Sense::Reversed,
                _ => {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("{}", "boundary segment sense is not 1 or 2"),
                    )?;
                    valid = false;
                    break;
                }
            };
            let Some(pcurve_count) = record.count(index + 2) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "boundary pcurve count is invalid"),
                )?;
                valid = false;
                break;
            };
            if (boundary_type == 0 && pcurve_count != 0)
                || (boundary_type == 1 && pcurve_count == 0)
            {
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "boundary pcurve collection cardinality disagrees with its representation type"))?;
                valid = false;
                break;
            }
            let mut pcurves = lookup_storage.with_storage(|| {
                ctx.collection_vec(pcurve_count, "iges Type141 segment pcurves")
            })?;
            for pcurve_index in ctx.admit_iter(0..pcurve_count, "iges Type141 pcurve traversal")? {
                let Some(pcurve) = pointer(record, index + 3 + pcurve_index) else {
                    ctx.clear_vec(&mut pcurves, "iges trimming rejected pcurves")?;
                    break;
                };
                if entry_by_sequence(entries, pcurve, ctx)?.is_none_or(|entry| {
                    entry.status.use_flag(global.global_table()) != Some(UseFlag::Parametric)
                }) {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("{}", "boundary pcurve does not have entity-use flag 05"),
                    )?;
                    ctx.clear_vec(&mut pcurves, "iges trimming rejected pcurves")?;
                    break;
                }
                pcurves.push(pcurve);
            }
            if pcurves.len() != pcurve_count {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "boundary pcurve pointer is invalid"),
                )?;
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
            lookup_storage.with_storage(|| {
                ctx.insert_btree_map(
                    &mut boundaries,
                    entry.sequence,
                    BoundaryDefinition { surface, segments },
                    "iges trimming boundary index nodes",
                )
            })?;
            ctx.insert_btree_set(
                &mut decoded,
                entry.sequence,
                "iges trimming decoded sequences",
            )?;
        }
    }
    for entry in ctx
        .admit_iter(directory, "iges trimming directory traversal")?
        .filter(|entry| matches!(entry.entity_type, 143 | 144) && entry.form == 0)
    {
        let mut sequence_storage =
            ctx.reserve_scoped(0, "iges trimming boundary sequence scratch")?;
        let factor = global.length_factor_mm();
        let carrier_agreement_tolerance = global.minimum_resolution_mm();
        let Some(record) = record_by_sequence(records, entry.sequence, ctx)? else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "Parameter Data record is missing"),
            )?;
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
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "trimmed-surface support pointer is invalid"),
                )?;
                continue;
            };
            let Some(has_explicit_outer) = record.integer(2).and_then(|value| match value {
                0 => Some(false),
                1 => Some(true),
                _ => None,
            }) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "trimmed-surface outer-boundary flag is not 0 or 1"),
                )?;
                continue;
            };
            let Some(inner_count) = record.count(3) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "trimmed-surface inner-boundary count is invalid"),
                )?;
                continue;
            };
            let sequence_count = inner_count
                .checked_add(usize::from(has_explicit_outer))
                .ok_or_else(|| {
                    ctx.refuse_codec_limit("iges Type144 boundary sequences", u64::MAX, 1)
                })?;
            let mut sequences = sequence_storage.with_storage(|| {
                ctx.collection_vec(sequence_count, "iges Type144 boundary sequences")
            })?;
            // The outer boundary is stated in its own PTO field, so it travels
            // as its own value and is never recovered from a list position.
            let mut explicit_outer_sequence = None;
            if has_explicit_outer {
                let Some(outer) = pointer(record, 4) else {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("{}", "trimmed-surface outer-boundary pointer is invalid"),
                    )?;
                    continue;
                };
                if entry_by_sequence(entries, outer, ctx)?
                    .is_none_or(|target| target.entity_type != 142 || target.form != 0)
                {
                    super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "trimmed-surface outer-boundary pointer does not target a Type 142 Form 0 entity"))?;
                    continue;
                }
                sequences.push(outer);
                explicit_outer_sequence = Some(outer);
            } else if !matches!(
                record.value(4),
                None | Some(TokenValue::Omitted | TokenValue::Integer(0))
            ) {
                super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "trimmed-surface parameter-domain outer-boundary pointer is neither zero nor omitted"))?;
                continue;
            }
            let mut valid = true;
            for index in ctx.admit_iter(0..inner_count, "iges Type144 inner traversal")? {
                let Some(sequence) = pointer(record, 5 + index) else {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("{}", "trimmed-surface inner-boundary pointer is invalid"),
                    )?;
                    valid = false;
                    break;
                };
                if entry_by_sequence(entries, sequence, ctx)?
                    .is_none_or(|target| target.entity_type != 142 || target.form != 0)
                {
                    super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "trimmed-surface inner-boundary pointer does not target a Type 142 Form 0 entity"))?;
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
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "bounded-surface representation type is not 0 or 1"),
                )?;
                continue;
            };
            let Some(surface) = pointer(record, 2) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "bounded-surface support pointer is invalid"),
                )?;
                continue;
            };
            let Some(count) = record.count(3).filter(|count| *count > 0) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "bounded-surface boundary count is not positive"),
                )?;
                continue;
            };
            let mut sequences = sequence_storage
                .with_storage(|| ctx.collection_vec(count, "iges Type143 boundary sequences"))?;
            let mut valid = true;
            for index in ctx.admit_iter(0..count, "iges Type143 boundary traversal")? {
                let Some(sequence) = pointer(record, 4 + index) else {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("{}", "bounded-surface boundary pointer is invalid"),
                    )?;
                    valid = false;
                    break;
                };
                if entry_by_sequence(entries, sequence, ctx)?
                    .is_none_or(|target| target.entity_type != 141 || target.form != 0)
                {
                    super::push_entity_loss(ctx, &mut losses, entry, format_args!("{}", "bounded-surface boundary pointer does not target a Type 141 Form 0 entity"))?;
                    valid = false;
                    break;
                }
                let representation_matches = match boundaries.get(&sequence) {
                    Some(boundary) => ctx.all_by(
                        &boundary.segments,
                        |segment| {
                            Ok(if representation == 0 {
                                segment.pcurves.is_empty()
                            } else {
                                !segment.pcurves.is_empty()
                            })
                        },
                        "iges bounded representation proof",
                    )?,
                    None => false,
                };
                if representation_matches {
                    sequences.push(sequence);
                } else {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!(
                            "{}",
                            "bounded-surface representation disagrees with its boundary"
                        ),
                    )?;
                    valid = false;
                    break;
                }
            }
            (surface, sequences, false, None, valid)
        };
        if !valid {
            continue;
        }
        let mut face_scratch = ctx.reserve_scoped(0, "iges trimming face scratch")?;
        let implicit_outer_domain = surface_kind == BoundarySurfaceKind::Trimmed
            && !has_explicit_outer
            && !boundary_sequences.is_empty();
        let surface_id = if implicit_outer_domain {
            face_scratch.with_storage(|| {
                crate::ids::surface_admitted(&crate::ids::Stem::directory(surface_sequence), ctx)
            })?
        } else {
            crate::ids::surface_admitted(&crate::ids::Stem::directory(surface_sequence), ctx)?
        };
        let copy_support = || {
            carrier_index
                .surfaces(surface_id.as_str(), ctx)?
                .map(|surface| match &surface.geometry {
                    SurfaceGeometry::Solved(solved) => solved
                        .try_clone_for_decode(ctx, "iges copied support surface")
                        .map(SurfaceGeometry::Solved),
                    SurfaceGeometry::Procedural {
                        cache: Some(solved),
                        ..
                    } => solved
                        .try_clone_for_decode(ctx, "iges copied support surface")
                        .map(SurfaceGeometry::Solved),
                    SurfaceGeometry::Procedural {
                        construction,
                        cache: None,
                    } => Ok(SurfaceGeometry::Procedural {
                        construction: construction
                            .try_clone_for_decode(ctx, "iges copied support construction ID")?,
                        cache: None,
                    }),
                })
                .transpose()
        };
        let Some(support_geometry) = (if implicit_outer_domain {
            copy_support()
        } else {
            face_scratch.with_storage(copy_support)
        })?
        else {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "trimmed-surface support carrier is missing"),
            )?;
            continue;
        };
        let mut candidate = ModelDraft::new();
        let stem = crate::ids::Stem::directory(entry.sequence);
        let body_id = crate::ids::body_admitted(&stem, ctx)?;
        sequences.record_body(&body_id, entry.sequence, &stem, ctx)?;
        let region_id = crate::ids::region_admitted(&stem, ctx)?;
        let shell_id = crate::ids::shell_admitted(&stem, ctx)?;
        let face_id = crate::ids::face_admitted(&stem, ctx)?;
        sequences.record_face(&face_id, entry.sequence, ctx)?;
        let mut candidate_boundary_vertex_derivations = Vec::new();
        let support_parameter_bounds = surface_parameter_bounds(&carrier_index, &surface_id, ctx)?;
        let support_parameter_intervals = surface_parameter_bound_intervals(
            support_parameter_bounds,
            &surface_id,
            entries,
            records,
            global.real_precision(),
            ctx,
        )?;
        let periodic_parameters = periodic_surface_parameters(&support_geometry);
        let mut implicit_boundary_curves = Vec::new();
        let mut implicit_boundary_pcurves = Vec::new();
        let mut loop_ids = Vec::new();
        let mut explicit_outer_loop: Option<cadmpeg_ir::ids::LoopId> = None;
        let mut linear_boundary_candidates = face_scratch.with_storage(|| {
            ctx.collection_vec(boundary_sequences.len(), "iges trimming linear candidates")
        })?;
        let mut face_tolerance = 0.0_f64;
        for (boundary_index, sequence) in ctx
            .admit_iter(&boundary_sequences, "iges trimming boundary traversal")?
            .copied()
            .enumerate()
        {
            let Some(boundary) = boundaries.get(&sequence) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "trimmed-surface boundary definition is missing"),
                )?;
                valid = false;
                break;
            };
            if boundary.surface != surface_sequence {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!(
                        "{}",
                        "boundary definition names a different support surface"
                    ),
                )?;
                valid = false;
                break;
            }
            let mut boundary_storage = ctx.reserve_scoped(0, "iges trimming boundary scratch")?;
            let mut items = boundary_storage.with_storage(|| {
                ctx.collection_vec(boundary.segments.len(), "iges trimming boundary items")
            })?;
            for segment in ctx.admit_iter(&boundary.segments, "iges trimming segment traversal")? {
                let model_curve_id = crate::ids::curve_admitted(
                    &crate::ids::Stem::directory(segment.model_curve),
                    ctx,
                )?;
                let Some(candidates) = ctx.get_btree_map(
                    &edges_by_curve,
                    &model_curve_id,
                    "iges boundary carrier query",
                )?
                else {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("{}", "boundary model curve has no bounded edge"),
                    )?;
                    valid = false;
                    break;
                };
                let mut pcurves = Some(boundary_storage.with_storage(|| {
                    ctx.collection_vec(segment.pcurves.len(), "iges trimming segment pcurves")
                })?);
                let mut pcurve_refusal = None;
                for sequence in
                    ctx.admit_iter(&segment.pcurves, "iges trimming pcurve traversal")?
                {
                    if composite_index.is_none() {
                        composite_index = Some(
                            composite_storage.with_storage(|| CompositeIndex::from_ir(ir, ctx))?,
                        );
                    }
                    let index = composite_index.as_ref().ok_or_else(|| {
                        CodecError::Malformed("IGES trimming composite index is absent".into())
                    })?;
                    match pcurve_geometry(
                        ir,
                        Some(&carrier_index),
                        *sequence,
                        &PcurveSupport {
                            surface_id: &surface_id,
                            geometry: &support_geometry,
                            factor,
                        },
                        Some(carrier_agreement_tolerance),
                        ctx,
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
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("boundary parameter curve states no NURBS carrier: {error}"),
                    )?;
                    valid = false;
                    break;
                }
                let mut pcurves = match pcurves {
                    Some(pcurves) => pcurves,
                    None if segment.parameter_curves_authoritative => {
                        super::push_entity_loss(
                            ctx,
                            &mut losses,
                            entry,
                            format_args!("{}", "boundary parameter curve has no NURBS carrier"),
                        )?;
                        valid = false;
                        break;
                    }
                    None => Vec::new(),
                };
                let pcurve_outside_support = ctx.any_by(
                    pcurves.iter().zip(&segment.pcurves),
                    |((geometry, range), sequence)| {
                        Ok(!pcurve_within_declared_intervals(
                            geometry,
                            *range,
                            support_parameter_intervals,
                            periodic_parameters,
                            ctx,
                        )? && !source_curve_control_polygon_within_bounds(
                            &carrier_index,
                            &boundary_storage.with_storage(|| {
                                crate::ids::curve_admitted(
                                    &crate::ids::Stem::directory(*sequence),
                                    ctx,
                                )
                            })?,
                            &PcurveSupport {
                                surface_id: &surface_id,
                                geometry: &support_geometry,
                                factor,
                            },
                            support_parameter_intervals,
                            (entries, records),
                            global.real_precision(),
                            ctx,
                        )?)
                    },
                    "iges trimming support pcurves",
                )?;
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
                    ctx.clear_vec(&mut pcurves, "iges trimming rejected pcurves")?;
                }
                let (source_edge, start, end, pcurves_agree) =
                    match boundary_storage.with_storage(|| {
                        select_boundary_edge(
                            candidates,
                            &carrier_index,
                            BoundaryMatch {
                                surface_id: &surface_id,
                                pcurves: &pcurves,
                                sense: segment.sense,
                                tolerance: carrier_agreement_tolerance,
                                parameter_curves_authoritative: segment
                                    .parameter_curves_authoritative,
                            },
                            ctx,
                        )
                    }) {
                        Ok(selected) => selected,
                        Err(BoundaryEdgeSelectionError::MissingEndpoints) => {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!("{}", "boundary model-curve endpoints are missing"),
                            )?;
                            valid = false;
                            break;
                        }
                        Err(BoundaryEdgeSelectionError::InvalidRange) => {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!(
                                "{}",
                                "boundary model-curve edge range does not evaluate to its vertices"
                            ),
                            )?;
                            valid = false;
                            break;
                        }
                        Err(BoundaryEdgeSelectionError::Ambiguous) => {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!(
                                "{}",
                                "boundary model curve maps to multiple ambiguous edge occurrences"
                            ),
                            )?;
                            valid = false;
                            break;
                        }
                        Err(BoundaryEdgeSelectionError::PcurveDisagreement) => {
                            super::push_entity_loss(
                                ctx,
                                &mut losses,
                                entry,
                                format_args!(
                                "{}",
                                "curve-on-surface carriers disagree beyond the minimum resolution"
                            ),
                            )?;
                            valid = false;
                            break;
                        }
                        Err(BoundaryEdgeSelectionError::Resource(error)) => return Err(error),
                    };
                if !pcurves_agree {
                    ctx.clear_vec(&mut pcurves, "iges trimming rejected pcurves")?;
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
                ctx.reserve_vec(
                    &mut implicit_boundary_curves,
                    items.len(),
                    "iges implicit boundary curve IDs",
                )?;
                for item in ctx.admit_iter(&items, "iges implicit boundary traversal")? {
                    implicit_boundary_curves.push(
                        item.model_curve
                            .try_clone_for_decode(ctx, "iges implicit boundary curve ID text")?,
                    );
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
                ctx.admit_iter(&items, "iges face coordinate endpoints")?
                    .flat_map(|item| [item.start.get(), item.end.get()]),
                ctx,
            )?;
            let sewing_tolerance = tolerance_policy.topology_sewing;
            face_tolerance = face_tolerance.max(sewing_tolerance);
            if ctx.any_by(
                items.iter().enumerate(),
                |(index, item)| {
                    let (_, end) = traversal(item);
                    let (next_start, _) = traversal(&items[(index + 1) % items.len()]);
                    Ok(!close(end.get(), next_start.get(), sewing_tolerance))
                },
                "iges boundary closure comparisons",
            )? {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "ordered boundary segments do not form a closed ring"),
                )?;
                valid = false;
                break;
            }
            linear_boundary_candidates.push(face_scratch.with_storage(|| {
                linear_boundary_geometry(
                    &items,
                    &carrier_index,
                    &support_geometry,
                    carrier_agreement_tolerance,
                    sewing_tolerance,
                    surface_kind,
                    ctx,
                )
            })?);
            let loop_id = crate::ids::loop_admitted(&stem.slot(boundary_index), ctx)?;
            let mut coedge_ids = ctx.collection_vec(items.len(), "iges trimming coedge ids")?;
            let endpoint_count = items.len().checked_mul(2).ok_or_else(|| {
                cadmpeg_core::decode::refuse_local_limit(
                    "iges trimming source endpoints",
                    u64::MAX,
                    1,
                )
            })?;
            let mut source_endpoints = boundary_storage.with_storage(|| {
                ctx.collection_vec(endpoint_count, "iges trimming source endpoints")
            })?;
            for (index, item) in ctx
                .admit_iter(&items, "iges trimming endpoint traversal")?
                .enumerate()
            {
                coedge_ids.push(crate::ids::coedge_admitted(
                    &stem.slot(boundary_index).slot(index),
                    ctx,
                )?);
                for (endpoint, position) in [
                    (BoundaryEndpoint::Start, item.start),
                    (BoundaryEndpoint::End, item.end),
                ] {
                    source_endpoints.push(BoundaryVertexSourceEndpoint {
                        edge: boundary_storage.with_storage(|| {
                            ctx.format_retained(
                                format_args!("{}", item.source_edge.id),
                                "iges trimming source endpoint edge text",
                            )
                        })?,
                        endpoint,
                        position,
                    });
                }
            }
            let Some(checked_sewing_tolerance) =
                cadmpeg_ir::scalar::PositiveReal::new(sewing_tolerance)
            else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "boundary sewing tolerance is invalid"),
                )?;
                valid = false;
                break;
            };
            let BoundaryVertices {
                ids: vertex_ids,
                derivations,
                _storage: _vertex_storage,
            } = match create_boundary_vertices(
                &mut candidate,
                &stem,
                (
                    &boundary_storage.with_storage(|| {
                        ctx.format_retained(
                            format_args!("iges:entity:directory#{}", entry.sequence),
                            "iges trimming source entity text",
                        )
                    })?,
                    boundary_index,
                ),
                &source_endpoints,
                checked_sewing_tolerance,
                sequences,
                ctx,
            ) {
                Ok(result) => result,
                Err(BoundaryVertexCreationError::Cluster(
                    BoundaryVertexClusterError::NonTransitive,
                )) => {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!(
                            "{}",
                            "boundary endpoint tolerance neighborhoods are non-transitive"
                        ),
                    )?;
                    valid = false;
                    break;
                }
                Err(BoundaryVertexCreationError::Resource(error)) => return Err(error),
            };
            ctx.extend_vec(
                &mut candidate_boundary_vertex_derivations,
                derivations,
                "iges trimming candidate vertex derivations",
            )?;
            for (segment_index, item) in ctx
                .admit_iter(items, "iges trimming edge traversal")?
                .enumerate()
            {
                let edge_id =
                    crate::ids::edge_admitted(&stem.slot(boundary_index).slot(segment_index), ctx)?;
                let start_vertex = vertex_ids[segment_index * 2]
                    .try_clone_for_decode(ctx, "iges trimming identity copy")?;
                let end_vertex = vertex_ids[segment_index * 2 + 1]
                    .try_clone_for_decode(ctx, "iges trimming identity copy")?;
                let carrier = match cadmpeg_ir::topology::EdgeCarrier::new(
                    Some(item.model_curve),
                    item.source_edge
                        .param_range()
                        .map(cadmpeg_ir::units::FiniteVector::get),
                ) {
                    Ok(carrier) => carrier,
                    Err(error) => {
                        super::push_entity_loss(ctx, &mut losses, entry, format_args!("{error}"))?;
                        valid = false;
                        break;
                    }
                };
                ctx.charge_entities(1, "iges_geometry_trimming")?;
                ctx.reserve_vec(
                    &mut candidate.model_mut().edges,
                    1,
                    "iges trimming edges slots",
                )?;
                candidate.model_mut().edges.push(Edge {
                    id: edge_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
                    carrier,
                    start: start_vertex,
                    end: end_vertex,
                    tolerance: Some(checked_sewing_tolerance),
                });
                if ctx.any_by(
                    &item.pcurves,
                    |(_, range)| Ok(cadmpeg_ir::units::FiniteVector::new(*range).is_none()),
                    "iges pcurve parameter range proof",
                )? {
                    super::push_entity_loss(
                        ctx,
                        &mut losses,
                        entry,
                        format_args!("{}", PcurveMetadata::NON_FINITE_PARAMETER_RANGE),
                    )?;
                    valid = false;
                    break;
                }
                let mut pcurve_uses =
                    ctx.collection_vec(item.pcurves.len(), "iges trimming coedge pcurve uses")?;
                for (pcurve_index, (geometry, parameter_range)) in ctx
                    .admit_iter(item.pcurves, "iges trimming pcurve use traversal")?
                    .enumerate()
                {
                    let id = crate::ids::pcurve_admitted(
                        &stem
                            .slot(boundary_index)
                            .slot(segment_index)
                            .slot(pcurve_index),
                        ctx,
                    )?;
                    if implicit_outer_domain {
                        ctx.reserve_vec(
                            &mut implicit_boundary_pcurves,
                            1,
                            "iges implicit boundary pcurve IDs",
                        )?;
                        implicit_boundary_pcurves.push(
                            id.try_clone_for_decode(ctx, "iges implicit boundary pcurve ID text")?,
                        );
                    }
                    ctx.reserve_vec(
                        &mut candidate.model_mut().pcurves,
                        1,
                        "iges trimming pcurve slots",
                    )?;
                    ctx.charge_entities(1, "iges_geometry_trimming")?;
                    candidate.model_mut().pcurves.push(Pcurve {
                        id: id.try_clone_for_decode(ctx, "iges trimming pcurve ID copy")?,
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
                let coedge_id = coedge_ids[segment_index]
                    .try_clone_for_decode(ctx, "iges trimming identity copy")?;
                ctx.charge_entities(1, "iges_geometry_trimming")?;
                ctx.reserve_vec(
                    &mut candidate.model_mut().coedges,
                    1,
                    "iges trimming coedges slots",
                )?;
                candidate.model_mut().coedges.push(Coedge {
                    id: coedge_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
                    owner_loop: loop_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
                    edge: edge_id,
                    radial_next: coedge_id,
                    sense: item.segment.sense,
                    pcurves: pcurve_uses,
                    use_curve: None,
                });
            }
            let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(ctx, coedge_ids, Vec::new())
                .map_err(cadmpeg_core::CodecError::from)?
            else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "boundary loop contains no coedges"),
                )?;
                valid = false;
                break;
            };
            ctx.charge_entities(1, "iges_geometry_trimming")?;
            ctx.reserve_vec(
                &mut candidate.model_mut().loops,
                1,
                "iges trimming loops slots",
            )?;
            candidate.model_mut().loops.push(Loop {
                id: loop_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
                face: face_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
            if explicit_outer_sequence == Some(sequence) {
                explicit_outer_loop =
                    Some(loop_id.try_clone_for_decode(ctx, "iges trimming identity copy")?);
            }
            ctx.reserve_vec(&mut loop_ids, 1, "iges trimming face loop IDs")?;
            loop_ids.push(loop_id);
        }
        if !valid {
            continue;
        }
        let linear_rings = match face_scratch.with_storage(|| {
            linear_boundary_rings(&linear_boundary_candidates, BoundarySpace::Parameter, ctx)
        })? {
            Some(rings) => Some(rings),
            None => face_scratch.with_storage(|| {
                linear_boundary_rings(&linear_boundary_candidates, BoundarySpace::Model, ctx)
            })?,
        };
        let linear_relationship = match linear_rings {
            Some(rings) => linear_boundary_relationship_is_valid(
                rings.as_deref(),
                surface_kind,
                has_explicit_outer,
                &support_geometry,
                support_parameter_bounds,
                periodic_parameters,
                ctx,
            )?,
            None => None,
        };
        if linear_relationship == Some(false) {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!(
                    "{}",
                    if surface_kind == BoundarySurfaceKind::Trimmed {
                        "trimmed-surface boundary loops are not simple, disjoint, and correctly nested"
                    } else {
                        "boundary loop is not a simple closed carrier"
                    }
                ),
            )?;
            continue;
        }
        let face_surface_id = if implicit_outer_domain {
            let derived_surface_id = crate::ids::surface_admitted(
                &crate::ids::Stem::directory(entry.sequence).part(crate::ids::Word::ImplicitOuter),
                ctx,
            )?;
            sequences.record_surface(&derived_surface_id, entry.sequence, ctx)?;
            ctx.charge_entities(1, "iges_geometry_trimming")?;
            ctx.reserve_vec(
                &mut candidate.model_mut().surfaces,
                1,
                "iges trimming surfaces slots",
            )?;
            candidate.model_mut().surfaces.push(Surface {
                id: derived_surface_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
                geometry: support_geometry,
                source_object: Some(match source_object(entry, ctx) {
                    Ok(source) => source,
                    Err(error) => {
                        super::push_entity_loss(
                            ctx,
                            &mut losses,
                            entry,
                            format_args!("{}", super::non_resource_error(error, ctx)?),
                        )?;
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
                    super::push_entity_loss(ctx, &mut losses, entry, format_args!("{error}"))?;
                    continue;
                }
            };

            ctx.charge_entities(1, "iges_geometry_trimming")?;
            let _attached = candidate.model_mut().add_procedural_surface(
                ctx,
                &derived_surface_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
                ProceduralSurface::new(
                    crate::ids::procedural_surface_admitted(
                        &crate::ids::Stem::directory(entry.sequence)
                            .part(crate::ids::Word::ImplicitOuter),
                        ctx,
                    )?,
                    ProceduralSurfaceDefinition::CurveBounded {
                        support: surface_id
                            .try_clone_for_decode(ctx, "iges trimming identity copy")?,
                        boundaries: implicit_boundary_curves,
                        boundary_pcurves: implicit_boundary_pcurves,
                        implicit_outer: true,
                    },
                    record_bounds,
                ),
            )?;
            derived_surface_id
        } else {
            surface_id
        };
        let checked_face_tolerance = if face_tolerance > 0.0 {
            let Some(value) = cadmpeg_ir::scalar::PositiveReal::new(face_tolerance) else {
                super::push_entity_loss(
                    ctx,
                    &mut losses,
                    entry,
                    format_args!("{}", "face tolerance is invalid"),
                )?;
                continue;
            };
            Some(value)
        } else {
            None
        };
        let face_loops = match explicit_outer_loop {
            Some(outer) => {
                ctx.retain_vec(
                    &mut loop_ids,
                    |id| {
                        Ok(!ctx.equal_bytes(
                            id.as_str().as_bytes(),
                            outer.as_str().as_bytes(),
                            "iges outer loop identity comparison",
                        )?)
                    },
                    "iges trimming inner loop IDs",
                )?;
                cadmpeg_ir::topology::FaceLoops::classified(outer, loop_ids)
            }
            None => cadmpeg_ir::topology::FaceLoops::unspecified(loop_ids),
        };
        ctx.charge_entities(1, "iges_geometry_trimming")?;
        ctx.reserve_vec(
            &mut candidate.model_mut().faces,
            1,
            "iges trimming faces slots",
        )?;
        candidate.model_mut().faces.push(Face {
            id: face_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            shell: shell_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            surface: face_surface_id,
            sense: Sense::Forward,
            loops: face_loops,
            name: None,
            color: None,
            tolerance: checked_face_tolerance,
        });
        let mut shell_faces = ctx.collection_vec(1, "iges trimming shell face IDs")?;
        shell_faces.push(face_id);
        ctx.charge_entities(1, "iges_geometry_trimming")?;
        let shell = match Shell::new(
            shell_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            region_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            shell_faces,
            Vec::new(),
            Vec::new(),
        ) {
            Ok(shell) => shell,
            Err(error) => {
                return Err(CodecError::Malformed(ctx.format_retained(
                    format_args!("{error}"),
                    "iges trimming shell error",
                )?))
            }
        };
        ctx.reserve_vec(
            &mut candidate.model_mut().shells,
            1,
            "iges trimming shells slots",
        )?;
        candidate.model_mut().shells.push(shell);
        let mut region_shells = ctx.collection_vec(1, "iges trimming region shell IDs")?;
        region_shells.push(shell_id);
        ctx.charge_entities(1, "iges_geometry_trimming")?;
        ctx.reserve_vec(
            &mut candidate.model_mut().regions,
            1,
            "iges trimming regions slots",
        )?;
        candidate.model_mut().regions.push(Region {
            id: region_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            body: body_id.try_clone_for_decode(ctx, "iges trimming identity copy")?,
            shells: region_shells,
        });
        let mut body_regions = ctx.collection_vec(1, "iges trimming body region IDs")?;
        body_regions.push(region_id);
        ctx.charge_entities(1, "iges_geometry_trimming")?;
        ctx.reserve_vec(
            &mut candidate.model_mut().bodies,
            1,
            "iges trimming bodies slots",
        )?;
        candidate.model_mut().bodies.push(Body {
            id: body_id,
            kind: BodyKind::Sheet,
            regions: body_regions,
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        candidate.model_mut().finalize(ctx)?;
        staged_storage
            .with_storage(|| ctx.reserve_vec(&mut staged, 1, "iges trimming staged candidates"))?;
        staged.push((entry, candidate, candidate_boundary_vertex_derivations));
    }
    drop(carrier_index);
    let mut commit_session = CommitSession::new(ir, ctx, None)?;
    for (entry, candidate, derivations) in
        ctx.admit_iter(staged, "iges trimming commit traversal")?
    {
        if commit_session.commit_model(candidate)?.is_err() {
            super::push_entity_loss(
                ctx,
                &mut losses,
                entry,
                format_args!("{}", "trimmed sheet candidate failed neutral validation"),
            )?;
            continue;
        }
        ctx.insert_btree_set(
            &mut decoded,
            entry.sequence,
            "iges trimming decoded sequences",
        )?;
        ctx.extend_vec(
            &mut boundary_vertex_derivations,
            derivations,
            "iges trimming committed vertex derivations",
        )?;
    }

    Ok((
        ProjectionOutcome { decoded, losses },
        boundary_vertex_derivations,
    ))
}

#[cfg(test)]
mod tests;
