// SPDX-License-Identifier: Apache-2.0
//! Topology emission, unresolved carriers, and source metadata.

use super::geometry_work::GeometryWorkBudget;
use super::jpeg::jpeg_dimensions;
use super::pcurves::{
    attach_tolerant_edge_intersections_with_budget,
    complete_exact_boundary_intersection_pcurves_with_budget,
    complete_intersection_pcurves_from_opposite_charts_with_budget,
    complete_tolerant_intersection_pcurves_from_serialized_branches_for_stream_with_budget,
    ordered_parameter_range, pcurve_endpoint_witness_with_index_and_budget,
    pcurve_matches_edge_range_with_index_and_budget, pcurve_parameter_range, EndpointWitnesses,
    IntersectionEntityStarts, IntersectionIncidenceIndex, TransferBudget,
};
use super::Scan;
use crate::decode::ids::IdScope;
use crate::framing::node_kind::NodeKind;
use crate::native::substrate::ParsedStreams;
use crate::parasolid::{Stream, StreamKind};
use crate::topology::{FaceLoop, FaceLoopFailure, Graph, Node, VertexFields};
use cadmpeg_core::decode::DecodeContext;
use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_core::CodecError;
use cadmpeg_ir::document::{CadIr, SourceMeta};
use cadmpeg_ir::eval::finite_or_refusal;
use cadmpeg_ir::features::geometry::FinitePoint3;
use cadmpeg_ir::geometry::intersection_curves::{IntcurveSupportContext, IntcurveSupportSide};
use cadmpeg_ir::geometry::legacy_cache::FitTolerance;
use cadmpeg_ir::geometry::pcurve::Pcurve;
use cadmpeg_ir::geometry::pcurve::{PcurveGeometry, PcurveMetadata};
use cadmpeg_ir::geometry::procedural_curves::ProceduralCurveDefinition;
use cadmpeg_ir::geometry::surface_curves::SurfaceCurveFamily;
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, ProceduralCurve, SolvedCurveGeometry, SolvedSurfaceGeometry, Surface,
    SurfaceGeometry,
};
use cadmpeg_ir::hash::{sha256, LowerHex};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, Identity, LoopId, PcurveId, PointId,
    ProceduralCurveId, RegionId, ShellId, SurfaceId, UnknownId, VertexId,
};
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::topology::{Body, Coedge, Edge, Face, Loop, Point, Region, Shell, Vertex};
use cadmpeg_ir::unknown::UnknownRecord;
use cadmpeg_ir::{AnnotationBuilder, Exactness};
use left_out::LeftOut;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;

mod left_out;

const EPS_EMIT_CANONICAL_TRIM_RANGE_E6: f64 = 1.0e-6;

type IntersectionPcurveIndex =
    BTreeMap<(CurveId, SurfaceId), (PcurveGeometry, [f64; 2], Option<FitTolerance>)>;

/// A face whose non-loop fields are decoded, held until its loops resolve so
/// the face is constructed once with its complete boundary.
struct PendingFace {
    xmt: u32,
    id: FaceId,
    shell: ShellId,
    surface: SurfaceId,
    sense: cadmpeg_ir::topology::Sense,
    tolerance: Option<cadmpeg_ir::scalar::PositiveLength>,
}

pub(super) struct TopologyStream<'inputs> {
    pub(super) stream_index: usize,
    pub(super) graph: &'inputs Graph,
    pub(super) points: &'inputs BTreeMap<u32, PointId>,
    pub(super) surfaces: &'inputs BTreeMap<u32, SurfaceId>,
    pub(super) reversed_analytic_surfaces: &'inputs BTreeSet<SurfaceId>,
    pub(super) curves: &'inputs BTreeMap<u32, CurveId>,
    pub(super) pcurves: &'inputs BTreeMap<u32, PcurveId>,
    pub(super) pcurve_supports: &'inputs BTreeMap<u32, SurfaceId>,
    pub(super) trim_ranges: &'inputs BTreeMap<u32, [f64; 2]>,
    pub(super) source_stream: &'inputs cadmpeg_ir::annotations::StreamHandle,
    pub(super) intersection_starts: IntersectionEntityStarts,
    pub(super) procedural_start: usize,
}

pub(super) struct TopologyBudgets<'inputs> {
    pub(super) exact_transfer: &'inputs TransferBudget<'inputs>,
    pub(super) completion_transfer: &'inputs TransferBudget<'inputs>,
    pub(super) adaptive_geometry: &'inputs GeometryWorkBudget<'inputs>,
    pub(super) completion_geometry: &'inputs GeometryWorkBudget<'inputs>,
}

/// Where topology emission charges its losses. A loss that states a face
/// partial is held with the face's id, so the caller reports it only for a
/// face the document still holds once inactive bodies are pruned.
pub(super) struct TopologyLosses<'losses> {
    pub(super) losses: &'losses mut Vec<cadmpeg_ir::report::loss::LossNote>,
    pub(super) partial_faces: &'losses mut Vec<(FaceId, cadmpeg_ir::report::loss::LossNote)>,
    pub(super) given_up: &'losses mut Vec<cadmpeg_ir::unknown::part::GivenUpPart>,
}

pub(super) fn emit_topology(
    ctx: &DecodeContext<'_>,
    ir: &mut CadIr,
    topology_stream: &TopologyStream<'_>,
    annotations: &mut AnnotationBuilder,
    intersection_index: &mut IntersectionIncidenceIndex,
    topology_budgets: &TopologyBudgets<'_>,
    topology_losses: TopologyLosses<'_>,
) -> Result<EndpointWitnesses, CodecError> {
    let TopologyLosses {
        losses: topology_losses,
        partial_faces: partial_face_losses,
        given_up,
    } = topology_losses;
    // This scope appends carriers and preserves their identity and construction links.
    let mut procedural_admission =
        cadmpeg_ir::document::procedural::ProceduralAdmission::new(&ir.geometry);

    let &TopologyBudgets {
        exact_transfer: exact_transfer_budget,
        completion_transfer: completion_transfer_budget,
        adaptive_geometry: adaptive_geometry_budget,
        completion_geometry: completion_geometry_budget,
    } = topology_budgets;

    let &TopologyStream {
        stream_index,
        graph,
        points,
        surfaces,
        reversed_analytic_surfaces,
        curves,
        pcurves,
        pcurve_supports,
        trim_ranges,
        source_stream,
        intersection_starts,
        procedural_start,
    } = topology_stream;

    let scope = IdScope::stream_from_index(stream_index)?;
    let mut left_out = LeftOut::new(ctx, (stream_index, source_stream.name()));
    let unresolved = |what: String| {
        crate::loss::NxLossCode::TopologyRecordUnresolved
            .note(format!("parasolid#{stream_index} {what}"))
    };
    let mut valid_face_xmts = BTreeSet::new();
    for shell in graph.body_shape_shells() {
        if let Some(faces) = graph.shell_face_xmts(shell) {
            for face in faces {
                valid_face_xmts.insert(face);
            }
        }
    }
    let mut face_boundaries: BTreeMap<u32, Vec<(u32, FaceLoop)>> = BTreeMap::new();
    let mut face_loop_failures: BTreeMap<u32, FaceLoopFailure> = BTreeMap::new();
    for face_xmt in &valid_face_xmts {
        match graph.face_loops(*face_xmt) {
            Ok(loops) => {
                face_boundaries.insert(*face_xmt, loops);
            }
            Err(failure) => {
                face_loop_failures.insert(*face_xmt, failure);
            }
        }
    }
    let mut point_positions: BTreeMap<PointId, Point3> = BTreeMap::new();
    for point in &ir.geometry.points {
        if !point_positions.contains_key(&point.id) {
            point_positions.insert(point.id.clone(), point.position().get());
        }
    }
    let vertex_point =
        |vertex_xmt: u32| resolved_vertex_point(graph, points, &point_positions, vertex_xmt);
    let mut partial_face_xmts: BTreeSet<u32> = BTreeSet::new();
    // Each loss names its face, so it waits until the face is emitted.
    let mut vertex_loop_losses = Vec::new();
    let mut omitted_vertex_loops = Vec::new();
    for (face_xmt, loops) in &mut face_boundaries {
        loops.retain(|(loop_xmt, face_loop)| {
            let FaceLoop::Vertex { fin_xmt, vertex } = face_loop else {
                return true;
            };
            let vertex_xmt = vertex.map(u32::from);
            if vertex_xmt.and_then(vertex_point).is_some() {
                return true;
            }
            vertex_loop_losses.push((*face_xmt, crate::loss::NxLossCode::TopologyVertexLoopUnresolved.note(
                format!("parasolid#{stream_index} LOOP {loop_xmt} of FACE {face_xmt} is omitted, and the face is stated partial: vertex-loop FIN {fin_xmt} names VERTEX {} without a resolved POINT", vertex_xmt.map_or_else(|| "null".to_owned(), |xmt| xmt.to_string())),
            )));
            partial_face_xmts.insert(*face_xmt);
            omitted_vertex_loops.push((*loop_xmt, vertex_xmt));
            false
        });
    }
    for (loop_xmt, vertex_xmt) in omitted_vertex_loops {
        let needs = match vertex_xmt.and_then(|xmt| graph.get(NodeKind::Vertex, xmt)) {
            Some(vertex) => Some(left_out.give_up(
                NodeKind::Vertex,
                vertex,
                unresolved(format!("VERTEX {} has no resolved POINT", vertex.xmt())),
            )?),
            None => None,
        };
        let Some(node) = graph.get(NodeKind::Loop, loop_xmt) else {
            continue;
        };
        match needs {
            Some(needs) => left_out.drop_with(
                (NodeKind::Loop, loop_xmt),
                scope.id_from_display::<Identity>(
                    &cadmpeg_ir::identity_component!("loop"),
                    loop_xmt,
                )?,
                &needs,
            ),
            None => {
                left_out.give_up(
                    NodeKind::Loop,
                    node,
                    unresolved(format!(
                        "LOOP {loop_xmt} is a vertex loop whose FIN names no VERTEX record"
                    )),
                )?;
            }
        }
    }
    let mut valid_loop_rings: BTreeMap<u32, &[u32]> = BTreeMap::new();
    let mut vertex_loop_vertices = BTreeSet::new();
    for loops in face_boundaries.values() {
        for (loop_xmt, face_loop) in loops {
            match face_loop {
                FaceLoop::Ring(ring) => {
                    valid_loop_rings.insert(*loop_xmt, ring.as_slice());
                }
                FaceLoop::Vertex { vertex, .. } => {
                    vertex_loop_vertices.extend(vertex.map(u32::from));
                }
            }
        }
    }
    let mut valid_fin_xmts = BTreeSet::new();
    for ring in valid_loop_rings.values() {
        for fin in *ring {
            valid_fin_xmts.insert(*fin);
        }
    }
    let mut valid_edge_xmts = BTreeSet::new();
    let mut valid_vertex_xmts = vertex_loop_vertices;
    for xmt in &valid_fin_xmts {
        if let Some(edge) = graph
            .get(NodeKind::Fin, *xmt)
            .and_then(Node::fin_fields)
            .and_then(|fields| fields.edge.map(u32::from))
        {
            valid_edge_xmts.insert(edge);
        }
        {
            let fields = graph.get(NodeKind::Fin, *xmt).and_then(Node::fin_fields);
            let partner_vertex = fields
                .filter(|fields| fields.other.is_some_and(|target| u32::from(target) > 1))
                .and_then(|fields| graph.get_target(NodeKind::Fin, fields.other))
                .and_then(Node::fin_fields)
                .and_then(|fields| fields.vertex.map(u32::from));
            for vertex in [
                fields.and_then(|fields| fields.vertex.map(u32::from)),
                partner_vertex,
            ]
            .into_iter()
            .flatten()
            .filter(|vertex| *vertex > 1)
            {
                valid_vertex_xmts.insert(vertex);
            }
        }
        if let Some(vertex) = graph
            .get(NodeKind::Fin, *xmt)
            .and_then(Node::vertex_loop_fin_fields)
            .and_then(|fields| fields.vertex.map(u32::from))
        {
            valid_vertex_xmts.insert(vertex);
        }
    }
    let mut admitted_shell_faces: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for face in graph
        .of_kind(NodeKind::Face)
        .filter(|face| valid_face_xmts.contains(&face.xmt()))
    {
        let Some(face_fields) = face.face_fields() else {
            continue;
        };
        let Some(shell) = face_fields.shell.map(u32::from) else {
            continue;
        };
        if face_fields
            .surface
            .is_some_and(|surface| surfaces.contains_key(&u32::from(surface)))
        {
            admitted_shell_faces
                .entry(shell)
                .or_default()
                .push(face.xmt());
        }
    }
    let mut admitted_shells = Vec::new();
    let mut shape_bodies = BTreeSet::new();
    let mut given_up_shells = BTreeMap::new();
    for shell in graph.body_shape_shells() {
        let body = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from));
        shape_bodies.extend(body);
        if admitted_shell_faces.contains_key(&shell.xmt()) {
            admitted_shells.push(shell);
        } else {
            let withheld = crate::loss::NxLossCode::TopologyShellWithheld.note(format!(
                "parasolid#{stream_index} SHELL {} is withheld: no FACE of the shell is \
                 admitted",
                shell.xmt()
            ));
            let part = super::build::give_up_record(
                ctx,
                (stream_index, source_stream.name()),
                shell,
                &withheld,
            )?;
            let id = Identity::from(part.id().clone());
            left_out.insert(NodeKind::Shell, shell.xmt(), id.clone());
            if let Some(body) = body {
                given_up_shells.entry(body).or_insert(id);
            }
            given_up.push(part);
            topology_losses.push(withheld);
        }
    }
    let mut first_body_shells = BTreeMap::new();
    for &shell in &admitted_shells {
        if let Some(body) = shell
            .shell_fields()
            .and_then(|fields| fields.body.map(u32::from))
        {
            first_body_shells.entry(body).or_insert(shell);
        }
    }
    let mut bodies: BTreeMap<u32, BodyId> = BTreeMap::new();
    for (body_xmt, shell) in first_body_shells {
        let id: BodyId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("body"), body_xmt)?;
        if let Some(node) = graph.get(NodeKind::Body, body_xmt) {
            annotate_node(annotations, &id, source_stream, node, "BODY");
        } else {
            annotations.note(
                &id,
                source_stream,
                cadmpeg_core::decode::u64_from_index(shell.pos()),
                Some("UNRESOLVED_BODY_REFERENCE"),
            );
            annotations.exactness(&id, Exactness::Unknown);
        }
        bodies.insert(body_xmt, id.clone());

        ir.geometry.bodies.push(Body {
            id,
            kind: cadmpeg_ir::topology::BodyKind::Solid,
            transform: None,
            name: None,
            color: None,
            visible: None,
            native_ref: None,
            unstated: None,
        });
    }

    let mut regions: BTreeMap<u32, (RegionId, BodyId)> = BTreeMap::new();
    let mut shells: BTreeMap<u32, ShellId> = BTreeMap::new();
    for &node in &admitted_shells {
        let mut withhold = |reason: String| {
            let id = left_out.give_up(
                NodeKind::Shell,
                node,
                crate::loss::NxLossCode::TopologyShellWithheld.note(format!(
                    "parasolid#{stream_index} SHELL {} is withheld: {reason}",
                    node.xmt()
                )),
            )?;
            if let Some(body) = node.shell_fields().and_then(|fields| fields.body) {
                given_up_shells.entry(u32::from(body)).or_insert(id);
            }
            Ok::<_, CodecError>(())
        };
        let Some(fields) = node.shell_fields() else {
            withhold("its fields do not decode".to_owned())?;
            continue;
        };
        let Some(body_ref) = fields
            .body
            .and_then(|target| bodies.get(&u32::from(target)))
        else {
            withhold("it names no BODY".to_owned())?;
            continue;
        };
        let body: BodyId = body_ref.clone();
        let Some(region_xmt) = fields.region.map(u32::from) else {
            withhold("it names no REGION".to_owned())?;
            continue;
        };
        let region_id = if let Some((region, owner)) = regions.get(&region_xmt) {
            if owner != &body {
                withhold(format!("its REGION {region_xmt} belongs to {owner}"))?;
                continue;
            }
            region.clone()
        } else {
            let region: RegionId =
                scope.id_from_display(&cadmpeg_ir::identity_component!("region"), region_xmt)?;
            if let Some(region_node) = graph.get(NodeKind::Region, region_xmt) {
                annotate_node(annotations, &region, source_stream, region_node, "REGION");
            } else {
                annotations.note(
                    &region,
                    source_stream,
                    cadmpeg_core::decode::u64_from_index(node.pos()),
                    Some("UNRESOLVED_REGION_REFERENCE"),
                );
                annotations.exactness(&region, Exactness::Unknown);
            }
            annotations.derived(&region, "body")?;

            ir.geometry
                .regions
                .push(Region::new(region.clone(), body.clone(), Vec::new()));
            regions.insert(region_xmt, (region.clone(), body.clone()));
            region
        };
        let shell_id: ShellId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("shell"), node.xmt())?;
        annotate_node(annotations, &shell_id, source_stream, node, "SHELL");
        let mut shell_faces = Vec::new();
        for face_xmt in admitted_shell_faces.get(&node.xmt()).into_iter().flatten() {
            shell_faces.push(
                scope.id_from_display::<FaceId>(
                    &cadmpeg_ir::identity_component!("face"),
                    face_xmt,
                )?,
            );
        }

        if shell_faces.is_empty() {
            return Err(cadmpeg_core::CodecError::Malformed(
                cadmpeg_ir::features::selections::BodySelectionError::Empty.to_string(),
            ));
        }
        ir.geometry
            .shells
            .push(Shell::new(shell_id.clone(), region_id.clone()));
        if let Some(parent) = ir
            .geometry
            .regions
            .iter_mut()
            .find(|candidate| candidate.id == region_id)
        {
            parent.push_shell(shell_id.clone());
        }
        shells.insert(node.xmt(), shell_id);
    }
    let mut shell_owners = BTreeSet::new();
    for region in ir
        .geometry
        .regions
        .iter()
        .filter(|region| region.stated_shells().next().is_some())
    {
        shell_owners.insert(region.body.clone());
    }
    let mut withheld_bodies = BTreeSet::new();
    for body_xmt in shape_bodies {
        let body = bodies.get(&body_xmt);
        if body.is_none_or(|body| !shell_owners.contains(body)) {
            let withheld = crate::loss::NxLossCode::TopologyShellWithheld.note(format!(
                "parasolid#{stream_index} BODY {body_xmt} is withheld: no SHELL of the body is emitted"
            ));
            if let Some(shell) = given_up_shells.get(&body_xmt) {
                let body: Option<BodyId> = match body {
                    Some(body) => Some(body.clone()),
                    None => graph
                        .get(NodeKind::Body, body_xmt)
                        .map(|_| {
                            scope
                                .id_from_display(&cadmpeg_ir::identity_component!("body"), body_xmt)
                        })
                        .transpose()?,
                };
                if let Some(body) = body {
                    topology_losses.push(cadmpeg_ir::unknown::part::dropped_with(body, shell));
                }
            } else if let Some(node) = graph.get(NodeKind::Body, body_xmt) {
                given_up.push(super::build::give_up_record(
                    ctx,
                    (stream_index, source_stream.name()),
                    node,
                    &withheld,
                )?);
            }
            topology_losses.push(withheld);
            withheld_bodies.extend(body.cloned());
        }
    }
    ir.geometry
        .bodies
        .retain(|body| !withheld_bodies.contains(&body.id));
    let mut vertices: BTreeMap<u32, VertexId> = BTreeMap::new();
    let mut vertex_positions: BTreeMap<VertexId, (Point3, Option<f64>)> = BTreeMap::new();
    for node in graph
        .of_kind(NodeKind::Vertex)
        .filter(|node| valid_vertex_xmts.contains(&node.xmt()))
    {
        let Some((fields, point_ref, point_position)) = vertex_point(node.xmt()) else {
            left_out.give_up(
                NodeKind::Vertex,
                node,
                unresolved(format!("VERTEX {} has no resolved POINT", node.xmt())),
            )?;
            continue;
        };
        let tolerance = decoded_tolerance(fields.tolerance());
        let vertex: VertexId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("vertex"), node.xmt())?;
        annotate_node(annotations, &vertex, source_stream, node, "VERTEX");
        if tolerance.is_some() {
            annotations
                .derived(&vertex, "tolerance")
                .map_err(cadmpeg_core::CodecError::from)?;
        }

        ir.geometry.vertices.push(Vertex {
            id: vertex.clone(),
            point: point_ref.clone(),
            tolerance,
            native_ref: None,
        });
        vertices.insert(node.xmt(), vertex.clone());
        vertex_positions.insert(
            vertex,
            (
                point_position,
                tolerance.map(cadmpeg_ir::scalar::PositiveLength::get),
            ),
        );
    }
    let mut pcurve_indices: BTreeMap<PcurveId, usize> = BTreeMap::new();
    for (index, pcurve) in ir.geometry.pcurves.iter().enumerate() {
        pcurve_indices.insert(pcurve.id.clone(), index);
    }
    let mut curve_indices: BTreeMap<CurveId, usize> = BTreeMap::new();
    for (index, curve) in ir.geometry.curves.iter().enumerate() {
        if !curve_indices.contains_key(&curve.id) {
            curve_indices.insert(curve.id.clone(), index);
        }
    }
    let mut procedural_curve_ids: BTreeSet<CurveId> = BTreeSet::new();
    for procedural in &ir.geometry.procedural_curves {
        if let Some(owner) = ir.geometry.procedural_curve_owner(&procedural.id) {
            procedural_curve_ids.insert(owner.clone());
        }
    }
    let mut curve_point_cache = CurvePointCache::default();
    // A vertex that emission builds for a record it may later leave out, with
    // that record.
    let mut vertex_needs: BTreeMap<VertexId, (NodeKind, u32)> = BTreeMap::new();
    let mut edges: BTreeMap<u32, EdgeId> = BTreeMap::new();
    let mut edges_against_source: BTreeSet<u32> = BTreeSet::new();
    for node in graph
        .of_kind(NodeKind::Edge)
        .filter(|node| valid_edge_xmts.contains(&node.xmt()))
    {
        let Some(fields) = node.edge_fields() else {
            left_out.give_up(
                NodeKind::Edge,
                node,
                unresolved(format!("EDGE {} has fields that do not decode", node.xmt())),
            )?;
            continue;
        };
        let Some(fin) = graph.get_target(NodeKind::Fin, fields.fin) else {
            left_out.give_up(
                NodeKind::Edge,
                node,
                unresolved(format!("EDGE {} names no FIN record", node.xmt())),
            )?;
            continue;
        };
        let Some(fin_fields) = fin.fin_fields() else {
            left_out.give_up(
                NodeKind::Edge,
                node,
                unresolved(format!(
                    "EDGE {} names FIN {} whose fields do not decode",
                    node.xmt(),
                    fin.xmt()
                )),
            )?;
            continue;
        };
        let edge_id = scope
            .id_from_display::<Identity>(&cadmpeg_ir::identity_component!("edge"), node.xmt())?;
        // A tolerant edge carries one curve per FIN, each on its own surface. A FIN whose surface
        // has no parameter conversion loses its pcurve, so the partner FIN supplies the carrier.
        let partner_curve = fin_fields
            .other
            .and_then(|target| graph.get(NodeKind::Fin, u32::from(target)))
            .and_then(Node::fin_fields)
            .and_then(|partner| partner.curve_xmt);
        let carrier_candidates = [fields.curve, fin_fields.curve_xmt, partner_curve]
            .into_iter()
            .flatten()
            .map(u32::from)
            .filter(|xmt| *xmt > 1)
            .collect::<Vec<_>>();
        let curve_xmt = carrier_candidates
            .iter()
            .copied()
            .find(|xmt| curves.contains_key(xmt) || pcurves.contains_key(xmt))
            .or_else(|| carrier_candidates.first().copied());
        let mut curve = curve_xmt.and_then(|xmt| curves.get(&xmt)).cloned();
        let mut param_range = curve_xmt.and_then(|xmt| trim_ranges.get(&xmt)).copied();
        if curve.is_none() {
            let lifted = (|| -> Result<Option<_>, CodecError> {
                let Some(xmt) = curve_xmt else {
                    return Ok(None);
                };
                let Some(pcurve_id) = pcurves.get(&xmt) else {
                    return Ok(None);
                };
                let Some(pcurve_index) = pcurve_indices.get(pcurve_id) else {
                    return Ok(None);
                };
                let Some(pcurve) = ir.geometry.pcurves.get(*pcurve_index) else {
                    return Ok(None);
                };
                let Some(surface_ref) = pcurve_supports.get(&xmt) else {
                    return Ok(None);
                };
                let surface = surface_ref.clone();
                let parameter_range = pcurve
                    .parameter_range()
                    .map(cadmpeg_ir::units::FiniteVector::get)
                    .or(param_range)
                    .or_else(|| pcurve_parameter_range(&pcurve.geometry));
                let Some(parameter_range) = parameter_range.and_then(ordered_parameter_range)
                else {
                    return Ok(None);
                };
                Ok(Some((
                    surface,
                    pcurve.geometry.clone(),
                    parameter_range,
                    pcurve.fit_tolerance(),
                )))
            })()?;
            if let Some((surface, pcurve, parameter_range, _fit_tolerance)) = lifted {
                let carrier: CurveId = scope.id_from_display(
                    &cadmpeg_ir::identity_component!("edge-parametric-curve"),
                    node.xmt(),
                )?;
                let construction: ProceduralCurveId = scope.id_from_display(
                    &cadmpeg_ir::identity_component!("edge-parametric-construction"),
                    node.xmt(),
                )?;
                annotations.note(
                    &carrier,
                    source_stream,
                    cadmpeg_core::decode::u64_from_index(node.pos()),
                    Some("PARAMETRIC_SURFACE_CURVE"),
                );
                annotations.derived(&carrier, "geometry")?;

                ir.geometry.curves.push(Curve {
                    parameter_range: None,
                    id: carrier.clone(),
                    geometry: CurveGeometry::Procedural {
                        construction: construction.clone(),
                        cache: None,
                    },
                    source_object: None,
                    native_ref: None,
                });

                let _attached = procedural_admission.add_curve(
                    &mut ir.geometry,
                    &carrier.clone(),
                    ProceduralCurve::new(
                        construction,
                        ProceduralCurveDefinition::SurfaceCurve {
                            family: SurfaceCurveFamily::Parametric {
                                context: IntcurveSupportContext::try_new(
                                    [
                                        IntcurveSupportSide {
                                            surface: Some(surface),
                                            pcurve: Some(pcurve.into()),
                                        },
                                        IntcurveSupportSide {
                                            surface: None,
                                            pcurve: None,
                                        },
                                    ],
                                    parameter_range,
                                    [Vec::new(), Vec::new(), Vec::new()],
                                )
                                .map_err(CodecError::malformed)?,
                                tail: None,
                            },
                        },
                    ),
                )?;
                curve = Some(carrier);
                param_range = None;
            }
        }
        let closed_edge = fin_fields.vertex.is_none()
            && fin_fields.forward.map(u32::from) == Some(fin.xmt())
            && fin_fields.backward.map(u32::from) == Some(fin.xmt());
        let start = fin_fields
            .vertex
            .and_then(|target| vertices.get(&u32::from(target)))
            .cloned();
        let synthesized_start = start.is_none() && closed_edge;
        let start = if start.is_some() || !closed_edge {
            start
        } else if let Some((curve, curve_index)) = curve.as_ref().and_then(|curve| {
            curve_indices
                .get(curve)
                .copied()
                .map(|index| (curve, index))
        }) {
            synthesize_closed_edge_vertex_with_curve_index_and_budget(
                ir,
                annotations,
                &ClosedEdge {
                    scope: &scope,
                    edge: node,
                    curve,
                    curve_index,
                    range: param_range,
                    source_stream,
                    tolerance: decoded_tolerance(fields.tolerance()),
                },
                &mut curve_point_cache,
                adaptive_geometry_budget,
            )?
        } else {
            None
        };
        let Some(start) = start else {
            left_out.drop_with_or_give_up(
                (NodeKind::Edge, node),
                edge_id,
                fin_fields
                    .vertex
                    .map(|target| (NodeKind::Vertex, u32::from(target))),
                || unresolved(format!("EDGE {} has no resolved start VERTEX", node.xmt())),
            )?;
            continue;
        };
        if synthesized_start {
            vertex_needs.insert(start.clone(), (NodeKind::Edge, node.xmt()));
        }
        let end_fin = fin_fields
            .other
            .filter(|target| u32::from(*target) > 1)
            .or(fin_fields.backward);
        let Some(end_fields) = graph
            .get_target(NodeKind::Fin, end_fin)
            .and_then(Node::fin_fields)
        else {
            left_out.give_up(
                NodeKind::Edge,
                node,
                unresolved(format!("EDGE {} names no resolved end FIN", node.xmt())),
            )?;
            continue;
        };
        let mut end = end_fields
            .vertex
            .and_then(|target| vertices.get(&u32::from(target)))
            .cloned();
        let partner_closes = (end_fields.forward == end_fin && end_fields.backward == end_fin)
            || (end_fields.forward.is_none() && end_fields.backward.is_none());
        if end.is_none() && end_fields.vertex.is_none() && partner_closes {
            // A partnered closed FIN repeats the null vertex and either closes its own
            // forward/backward links or, on a free edge of a sheet, has no links at all.
            // Its endpoint is the same analytic point as the current FIN's synthesized
            // start, even when `end_fin` is a distinct radial partner record.
            end = Some(start.clone());
        }
        let Some(end) = end else {
            left_out.drop_with_or_give_up(
                (NodeKind::Edge, node),
                edge_id,
                end_fields
                    .vertex
                    .map(|target| (NodeKind::Vertex, u32::from(target))),
                || unresolved(format!("EDGE {} has no resolved end VERTEX", node.xmt())),
            )?;
            continue;
        };
        // FIN.vertex closes its FIN: `start` is where EDGE.fin ends and `end`
        // is where it begins. The IR edge runs along its curve.
        let curve_forward = fields
            .curve
            .filter(|target| u32::from(*target) > 1)
            .and_then(|target| curve_sense(graph, u32::from(target)))
            != Some(cadmpeg_ir::topology::Sense::Reversed);
        let (mut start, mut end) =
            if (fin_fields.sense == cadmpeg_ir::topology::Sense::Forward) == curve_forward {
                (end, start)
            } else {
                (start, end)
            };
        let mut against_source = !curve_forward;
        let id: EdgeId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("edge"), node.xmt())?;
        annotate_node(annotations, &id, source_stream, node, "EDGE");
        if decoded_tolerance(fields.tolerance()).is_some() {
            annotations
                .derived(&id, "tolerance")
                .map_err(cadmpeg_core::CodecError::from)?;
        }
        if let (Some(carrier), Some(range)) = (&curve, param_range) {
            let oriented = if let Some((
                curve_index,
                (start_position, start_tolerance),
                (end_position, end_tolerance),
            )) = curve_indices
                .get(carrier)
                .copied()
                .zip(vertex_positions.get(&start).copied())
                .zip(vertex_positions.get(&end).copied())
                .map(|((curve_index, start), end)| (curve_index, start, end))
            {
                orient_edge_range_for_geometry_with_budget(
                    &EdgeGeometryRange {
                        geometry: &ir.geometry.curves[curve_index].geometry,
                        curve: carrier,
                        range,
                        start_position,
                        start_tolerance,
                        end_position,
                        end_tolerance,
                        edge_tolerance: decoded_tolerance(fields.tolerance())
                            .map(cadmpeg_ir::scalar::PositiveLength::get),
                        procedural_curve: procedural_curve_ids.contains(carrier),
                    },
                    &mut curve_point_cache,
                    adaptive_geometry_budget,
                )?
            } else {
                None
            };
            match oriented {
                Some((oriented, reverse_edge)) => {
                    param_range = Some(oriented);
                    if reverse_edge {
                        std::mem::swap(&mut start, &mut end);
                        against_source = !against_source;
                    }
                }
                None => {
                    param_range = None;
                }
            }
        }
        // The stored senses order an edge without a range only relative to its curve record, so
        // its vertices can still sit on an open spline in the reverse order.
        if param_range.is_none() && start != end {
            let runs_against_curve = curve
                .as_ref()
                .and_then(|carrier| curve_indices.get(carrier).copied())
                .zip(vertex_positions.get(&start).copied())
                .zip(vertex_positions.get(&end).copied())
                .is_some_and(|((curve_index, (start_position, _)), (end_position, _))| {
                    open_spline_runs_from_end_to_start(
                        &ir.geometry.curves[curve_index].geometry,
                        start_position,
                        end_position,
                    )
                });
            if runs_against_curve {
                std::mem::swap(&mut start, &mut end);
                against_source = !against_source;
            }
        }

        ir.geometry.edges.push(Edge {
            id: id.clone(),
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(curve, param_range)
                .map_err(CodecError::malformed)?,
            ends: Some(cadmpeg_ir::topology::EdgeEnds::new(start, end)),
            tolerance: decoded_tolerance(fields.tolerance()),
            native_ref: None,
        });
        edges.insert(node.xmt(), id);
        if against_source {
            edges_against_source.insert(node.xmt());
        }
    }
    let mut edge_curves_by_id: BTreeMap<EdgeId, CurveId> = BTreeMap::new();
    for edge in &ir.geometry.edges {
        if let Some(curve) = edge.curve() {
            edge_curves_by_id.insert(edge.id.clone(), curve.clone());
        }
    }
    let mut faces: BTreeMap<u32, FaceId> = BTreeMap::new();
    let mut pending_faces: Vec<PendingFace> = Vec::new();
    for node in graph
        .of_kind(NodeKind::Face)
        .filter(|node| valid_face_xmts.contains(&node.xmt()))
    {
        let Some(fields) = node.face_fields() else {
            left_out.give_up(
                NodeKind::Face,
                node,
                unresolved(format!("FACE {} has fields that do not decode", node.xmt())),
            )?;
            continue;
        };
        let Some(shell_ref) = fields
            .shell
            .and_then(|target| shells.get(&u32::from(target)))
        else {
            left_out.drop_with_or_give_up(
                (NodeKind::Face, node),
                scope.id_from_display::<Identity>(
                    &cadmpeg_ir::identity_component!("face"),
                    node.xmt(),
                )?,
                fields
                    .shell
                    .map(|target| (NodeKind::Shell, u32::from(target))),
                || unresolved(format!("FACE {} names no emitted SHELL", node.xmt())),
            )?;
            continue;
        };
        let Some(surface_ref) = fields
            .surface
            .and_then(|target| surfaces.get(&u32::from(target)))
        else {
            left_out.give_up(
                NodeKind::Face,
                node,
                unresolved(format!("FACE {} names no decoded SURFACE", node.xmt())),
            )?;
            continue;
        };
        let id: FaceId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("face"), node.xmt())?;
        annotate_node(annotations, &id, source_stream, node, "FACE");
        if decoded_tolerance(fields.tolerance()).is_some() {
            annotations
                .derived(&id, "tolerance")
                .map_err(cadmpeg_core::CodecError::from)?;
        }

        let reversed_surface = reversed_analytic_surfaces.contains(surface_ref);
        if reversed_surface {
            annotations.derived(&id, "sense")?;
        }
        let sense = match (fields.sense, reversed_surface) {
            (cadmpeg_ir::topology::Sense::Forward, true)
            | (cadmpeg_ir::topology::Sense::Reversed, false) => {
                cadmpeg_ir::topology::Sense::Reversed
            }
            _ => cadmpeg_ir::topology::Sense::Forward,
        };
        pending_faces.push(PendingFace {
            xmt: node.xmt(),
            id: id.clone(),
            shell: shell_ref.clone(),
            surface: surface_ref.clone(),
            sense,
            tolerance: decoded_tolerance(fields.tolerance()),
        });
        faces.insert(node.xmt(), id);
    }
    leave_out_unresolved_boundaries(
        &mut left_out,
        (graph, &scope),
        &face_loop_failures,
        (&valid_edge_xmts, &valid_vertex_xmts),
        &unresolved,
    )?;
    let mut loops: BTreeMap<u32, LoopId> = BTreeMap::new();
    let mut loop_specs: BTreeMap<u32, (LoopId, FaceId)> = BTreeMap::new();
    let mut partial_faces: BTreeSet<FaceId> = BTreeSet::new();
    for &loop_xmt in valid_loop_rings.keys() {
        let Some(node) = graph.get(NodeKind::Loop, loop_xmt) else {
            continue;
        };
        let Some(fields) = node.loop_fields() else {
            continue;
        };
        let id: LoopId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("loop"), node.xmt())?;
        let Some(face_ref) = fields.face.and_then(|target| faces.get(&u32::from(target))) else {
            left_out.drop_with_or_give_up(
                (NodeKind::Loop, node),
                id,
                fields
                    .face
                    .map(|target| (NodeKind::Face, u32::from(target))),
                || unresolved(format!("LOOP {loop_xmt} names no emitted FACE")),
            )?;
            continue;
        };
        // The first FIN of the ring that does not resolve, with the record it
        // names and emission left out.
        let unresolved_fin = valid_loop_rings[&loop_xmt].iter().find_map(|fin_xmt| {
            let Some(fin) = graph.get(NodeKind::Fin, *fin_xmt) else {
                return Some(None);
            };
            if let Some(vertex_fin) = fin.vertex_loop_fin_fields() {
                let vertex = vertex_fin.vertex.map(u32::from);
                return (!vertex.is_some_and(|xmt| vertices.contains_key(&xmt)))
                    .then(|| vertex.map(|xmt| (NodeKind::Vertex, xmt)));
            }
            let edge = fin
                .fin_fields()
                .and_then(|fields| fields.edge.map(u32::from));
            (!edge.is_some_and(|xmt| edges.contains_key(&xmt)))
                .then(|| edge.map(|xmt| (NodeKind::Edge, xmt)))
        });
        if let Some(needs) = unresolved_fin {
            partial_face_losses.push((face_ref.clone(), crate::loss::NxLossCode::TopologyLoopRingUnresolved.note(
                format!("parasolid#{stream_index} LOOP {loop_xmt} of {face_ref} states no resolvable coedge ring: loop {id} is omitted, and face {face_ref} is stated partial"),
            )));
            partial_faces.insert(face_ref.clone());
            left_out.drop_with_or_give_up((NodeKind::Loop, node), id, needs, || {
                unresolved(format!(
                    "LOOP {loop_xmt} has a FIN that names no emitted EDGE"
                ))
            })?;
            continue;
        }
        annotate_node(annotations, &id, source_stream, node, "LOOP");
        loop_specs.insert(node.xmt(), (id.clone(), face_ref.clone()));
        loops.insert(node.xmt(), id);
    }
    let mut fin_ids = BTreeMap::new();
    for xmt in &valid_fin_xmts {
        if graph
            .get(NodeKind::Fin, *xmt)
            .and_then(Node::fin_fields)
            .is_some_and(|fields| {
                fields
                    .loop_xmt
                    .is_some_and(|target| loops.contains_key(&u32::from(target)))
            })
        {
            fin_ids.insert(
                *xmt,
                scope.id_from_display::<CoedgeId>(&cadmpeg_ir::identity_component!("fin"), xmt)?,
            );
        } else if let Some(node) = graph.get(NodeKind::Fin, *xmt) {
            left_out.drop_with_or_give_up(
                (NodeKind::Fin, node),
                scope.id_from_display::<Identity>(&cadmpeg_ir::identity_component!("fin"), xmt)?,
                node.fin_fields()
                    .and_then(|fields| fields.loop_xmt)
                    .map(|target| (NodeKind::Loop, u32::from(target))),
                || unresolved(format!("FIN {xmt} names no emitted LOOP")),
            )?;
        }
    }
    // Preserve the endpoint proof only when the admitted carrier is the exact
    // intersection candidate consumed by the later attachment pass. A valid
    // unrelated coedge pcurve must not become a general admission shortcut.
    let mut endpoint_witnesses = EndpointWitnesses::new();
    let mut intersection_pcurves = IntersectionPcurveIndex::new();
    for procedural in &ir.geometry.procedural_curves {
        let ProceduralCurveDefinition::Intersection { context, .. } = procedural.definition()
        else {
            continue;
        };
        let Some(owner) = ir.geometry.procedural_curve_owner(&procedural.id) else {
            continue;
        };
        for side in context.sides() {
            let (Some(surface), Some(pcurve)) = (&side.surface, &side.pcurve) else {
                continue;
            };
            intersection_pcurves.insert(
                (owner.clone(), surface.clone()),
                (
                    pcurve.geometry.clone(),
                    context.parameter_range().endpoints(),
                    procedural.cache_fit_tolerance(),
                ),
            );
        }
    }
    let (valid_pcurve_fins, fallback_pcurves) = {
        let index = cadmpeg_ir::index::ModelIndex::new_model_only(ir);
        let valid_pcurve_fins = fin_ids
            .keys()
            .map(|fin_xmt| -> Result<Option<u32>, CodecError> {
                let candidate = (|| {
                    let fields = graph.get(NodeKind::Fin, *fin_xmt)?.fin_fields()?;
                    let edge = fields
                        .edge
                        .and_then(|target| edges.get(&u32::from(target)))?;
                    let support = graph
                        .get_target(NodeKind::Loop, fields.loop_xmt)
                        .and_then(Node::loop_fields)
                        .and_then(|loop_| graph.get_target(NodeKind::Face, loop_.face))
                        .and_then(Node::face_fields)
                        .and_then(|face| {
                            face.surface
                                .and_then(|target| surfaces.get(&u32::from(target)))
                        })?;
                    let carrier_xmt = u32::from(fields.curve_xmt?);
                    if pcurve_supports.get(&carrier_xmt) != Some(support) {
                        return None;
                    }
                    let carrier_id = pcurves.get(&carrier_xmt)?;
                    let use_range = trim_ranges
                        .get(&carrier_xmt)
                        .copied()
                        .and_then(ordered_parameter_range);
                    Some((edge, support, carrier_id, use_range))
                })();
                let Some((edge, support, carrier_id, use_range)) = candidate else {
                    return Ok(None);
                };
                let Some(carrier) =
                    index.pcurves(carrier_id.as_str(), cadmpeg_ir::index::StandardIndex)
                else {
                    return Ok(None);
                };
                let parameter_range = use_range
                    .or(carrier
                        .parameter_range()
                        .map(cadmpeg_ir::units::FiniteVector::get))
                    .or_else(|| pcurve_parameter_range(&carrier.geometry));
                let Some(endpoints) = pcurve_endpoint_witness_with_index_and_budget(
                    &index,
                    edge,
                    support,
                    &carrier.geometry,
                    parameter_range,
                    carrier
                        .fit_tolerance()
                        .map(cadmpeg_ir::geometry::legacy_cache::FitTolerance::get),
                    adaptive_geometry_budget,
                )?
                else {
                    return Ok(None);
                };
                let Some(curve) = index
                    .edges(edge.as_str(), cadmpeg_ir::index::StandardIndex)
                    .and_then(|edge| edge.curve())
                else {
                    return Ok(None);
                };
                let Some(parameter_range) = parameter_range else {
                    return Ok(None);
                };
                let Some((candidate_geometry, candidate_range, _)) =
                    intersection_pcurves.get(&(curve.clone(), support.clone()))
                else {
                    return Ok(Some(*fin_xmt));
                };
                if *candidate_geometry != carrier.geometry || *candidate_range != parameter_range {
                    return Ok(Some(*fin_xmt));
                }
                let key = (curve.clone(), support.clone());

                let witnesses = match endpoint_witnesses.entry(key) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
                };

                witnesses.push((carrier.geometry.clone(), parameter_range, endpoints));
                Ok(Some(*fin_xmt))
            })
            .try_fold(BTreeSet::new(), |mut values, candidate| {
                if let Some(fin_xmt) = candidate? {
                    values.insert(fin_xmt);
                }
                Ok::<_, CodecError>(values)
            })?;
        let fallback_pcurves = fin_ids
            .keys()
            .map(|fin_xmt| -> Result<Option<_>, CodecError> {
                if valid_pcurve_fins.contains(fin_xmt) {
                    return Ok(None);
                }
                let candidate = (|| -> Result<Option<_>, CodecError> {
                    let Some(fields) = graph
                        .get(NodeKind::Fin, *fin_xmt)
                        .and_then(Node::fin_fields)
                    else {
                        return Ok(None);
                    };
                    let Some(edge) = fields.edge.and_then(|target| edges.get(&u32::from(target)))
                    else {
                        return Ok(None);
                    };
                    let Some(support_ref) = graph
                        .get_target(NodeKind::Loop, fields.loop_xmt)
                        .and_then(Node::loop_fields)
                        .and_then(|loop_| graph.get_target(NodeKind::Face, loop_.face))
                        .and_then(Node::face_fields)
                        .and_then(|face| {
                            face.surface
                                .and_then(|target| surfaces.get(&u32::from(target)))
                        })
                    else {
                        return Ok(None);
                    };
                    let Some(carrier) = edge_curves_by_id.get(edge) else {
                        return Ok(None);
                    };
                    let support: SurfaceId = support_ref.clone();
                    let lookup = (carrier.clone(), support.clone());
                    let Some((geometry, parameter_range, fit_tolerance)) =
                        intersection_pcurves.get(&lookup)
                    else {
                        return Ok(None);
                    };
                    Ok(Some((
                        edge,
                        support,
                        geometry.clone(),
                        *parameter_range,
                        *fit_tolerance,
                    )))
                })()?;
                let Some((edge, support, geometry, parameter_range, fit_tolerance)) = candidate
                else {
                    return Ok(None);
                };
                Ok(pcurve_matches_edge_range_with_index_and_budget(
                    &index,
                    edge,
                    &support,
                    &geometry,
                    None,
                    fit_tolerance.map(cadmpeg_ir::geometry::legacy_cache::FitTolerance::get),
                    adaptive_geometry_budget,
                )?
                .then_some((
                    *fin_xmt,
                    (support, geometry, parameter_range, fit_tolerance),
                )))
            })
            .try_fold(BTreeMap::new(), |mut values, candidate| {
                if let Some((fin_xmt, pcurve)) = candidate? {
                    values.insert(fin_xmt, pcurve);
                }
                Ok::<_, CodecError>(values)
            })?;
        (valid_pcurve_fins, fallback_pcurves)
    };
    let mut serialized_branch_pcurves = BTreeSet::new();
    for &fin_xmt in fin_ids.keys() {
        let Some(node) = graph.get(NodeKind::Fin, fin_xmt) else {
            continue;
        };
        let Some(fields) = node.fin_fields() else {
            continue;
        };
        let Some(loop_id_ref) = fields
            .loop_xmt
            .and_then(|target| loops.get(&u32::from(target)))
        else {
            continue;
        };
        let Some(edge_ref) = fields.edge.and_then(|target| edges.get(&u32::from(target))) else {
            continue;
        };
        let Some(id_ref) = fin_ids.get(&node.xmt()) else {
            continue;
        };
        let loop_id: LoopId = loop_id_ref.clone();
        let edge: EdgeId = edge_ref.clone();
        let id: CoedgeId = id_ref.clone();
        annotate_node(annotations, &id, source_stream, node, "FIN");
        let partner = fields
            .other
            .and_then(|target| fin_ids.get(&u32::from(target)))
            .cloned();
        let radial_next = partner.as_ref().unwrap_or(&id).clone();
        let support = graph
            .get_target(NodeKind::Loop, fields.loop_xmt)
            .and_then(Node::loop_fields)
            .and_then(|loop_| graph.get_target(NodeKind::Face, loop_.face))
            .and_then(Node::face_fields)
            .and_then(|face| {
                face.surface
                    .and_then(|target| surfaces.get(&u32::from(target)))
            })
            .cloned();
        let pcurve_use_range = fields
            .curve_xmt
            .and_then(|target| trim_ranges.get(&u32::from(target)))
            .copied()
            .and_then(ordered_parameter_range);
        let mut pcurve = if valid_pcurve_fins.contains(&node.xmt()) {
            fields
                .curve_xmt
                .and_then(|target| pcurves.get(&u32::from(target)))
                .cloned()
        } else {
            None
        };
        let edge_curve = edge_curves_by_id.get(&edge);
        if let (Some(pcurve), Some(edge_curve), Some(support)) =
            (pcurve.as_ref(), edge_curve, support.as_ref())
        {
            if fields
                .curve_xmt
                .and_then(|target| curves.get(&u32::from(target)))
                == Some(edge_curve)
            {
                serialized_branch_pcurves.insert((
                    edge_curve.clone(),
                    support.clone(),
                    pcurve.clone(),
                ));
            }
        }
        let attached_pcurve_use_range = pcurve.as_ref().and(pcurve_use_range);
        if pcurve.is_none() {
            if let Some((_support, geometry, parameter_range, fit_tolerance)) =
                fallback_pcurves.get(&fin_xmt)
            {
                let pcurve_id: PcurveId = scope.id_from_display(
                    &cadmpeg_ir::identity_component!("intersection-pcurve"),
                    fin_xmt,
                )?;
                annotations.note(
                    &pcurve_id,
                    source_stream,
                    cadmpeg_core::decode::u64_from_index(node.pos()),
                    Some("INTERSECTION_PCURVE"),
                );
                annotations.derived(&pcurve_id, "geometry")?;
                annotations.derived(&pcurve_id, "metadata.form.parameter_range")?;
                if fit_tolerance.is_some() {
                    annotations.derived(&pcurve_id, "metadata.form.fit_tolerance")?;
                }

                ir.geometry.pcurves.push(Pcurve {
                    id: pcurve_id.clone(),
                    geometry: geometry.clone(),
                    metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                        None,
                        Some(
                            cadmpeg_ir::units::FiniteVector::new(*parameter_range)
                                .ok_or(PcurveMetadata::NON_FINITE_PARAMETER_RANGE)
                                .map_err(cadmpeg_core::CodecError::malformed)?,
                        ),
                        *fit_tolerance,
                    ),
                    native_ref: None,
                });
                pcurve = Some(pcurve_id);
            }
        }

        let mut pcurve_uses = Vec::new();
        if let Some(pcurve) = pcurve {
            pcurve_uses.push(cadmpeg_ir::topology::PcurveUse {
                pcurve,
                isoparametric: None,
                parameter_range: attached_pcurve_use_range
                    .map(cadmpeg_ir::geometry::procedural_curves::DirectedParameterRange::new)
                    .transpose()
                    .map_err(CodecError::malformed)?,
            });
        }
        let sense = if fields
            .edge
            .is_some_and(|target| edges_against_source.contains(&u32::from(target)))
        {
            reversed(fields.sense)
        } else {
            fields.sense
        };
        ir.geometry.coedges.push(Coedge {
            id: id.clone(),
            owner_loop: loop_id.clone(),
            edge,
            radial_next,
            sense,
            pcurves: pcurve_uses,
            use_curve: None,
            native_ref: None,
        });
    }
    let mut face_loops: BTreeMap<FaceId, Vec<LoopId>> = BTreeMap::new();
    for (face_xmt, boundary) in &face_boundaries {
        for (loop_xmt, face_loop) in boundary {
            let fin_xmts = match face_loop {
                FaceLoop::Ring(fin_xmts) => fin_xmts,
                FaceLoop::Vertex { vertex, .. } => {
                    let Some(node) = graph.get(NodeKind::Loop, *loop_xmt) else {
                        continue;
                    };
                    let vertex_xmt = vertex.map(u32::from);
                    let vertex = vertex_xmt.and_then(|xmt| vertices.get(&xmt));
                    let (Some(face), Some(vertex)) = (faces.get(face_xmt), vertex) else {
                        let needs = if faces.contains_key(face_xmt) {
                            vertex_xmt.map(|xmt| (NodeKind::Vertex, xmt))
                        } else {
                            Some((NodeKind::Face, *face_xmt))
                        };
                        left_out.drop_with_or_give_up(
                            (NodeKind::Loop, node),
                            scope.id_from_display::<Identity>(
                                &cadmpeg_ir::identity_component!("loop"),
                                *loop_xmt,
                            )?,
                            needs,
                            || {
                                unresolved(format!(
                                    "LOOP {loop_xmt} is a vertex loop of no emitted FACE"
                                ))
                            },
                        )?;
                        if let Some(vertex) = vertex {
                            vertex_needs
                                .entry(vertex.clone())
                                .or_insert((NodeKind::Loop, *loop_xmt));
                        }
                        continue;
                    };
                    let id: LoopId = scope
                        .id_from_display(&cadmpeg_ir::identity_component!("loop"), *loop_xmt)?;
                    annotate_node(annotations, &id, source_stream, node, "LOOP");
                    ir.geometry.loops.push(Loop {
                        id: id.clone(),
                        face: face.clone(),
                        boundary: cadmpeg_ir::topology::LoopBoundary::Vertex {
                            vertex: vertex.clone(),
                            pcurves: Vec::new(),
                        },
                        native_ref: None,
                    });
                    face_loops.entry(face.clone()).or_default().push(id);
                    continue;
                }
            };
            let Some((id, face)) = loop_specs.get(loop_xmt) else {
                continue;
            };
            let mut coedges = Vec::new();
            let mut all_resolved = true;
            for fin_xmt in fin_xmts {
                let Some(fin_id) = fin_ids.get(fin_xmt) else {
                    all_resolved = false;
                    break;
                };

                coedges.push(fin_id.clone());
            }
            let ring = if all_resolved {
                cadmpeg_ir::topology::LoopRing::new(coedges, Vec::new()).ok()
            } else {
                None
            };
            let Some(ring) = ring else {
                partial_face_losses.push((face.clone(), crate::loss::NxLossCode::TopologyLoopRingUnresolved.note(
                    format!("parasolid#{stream_index} LOOP {loop_xmt} of {face} states no resolvable coedge ring: loop {id} is omitted, and face {face} is stated partial"),
                )));
                partial_faces.insert(face.clone());
                if let Some(node) = graph.get(NodeKind::Loop, *loop_xmt) {
                    let needs = fin_xmts
                        .iter()
                        .find(|fin_xmt| !fin_ids.contains_key(*fin_xmt))
                        .map(|fin_xmt| (NodeKind::Fin, *fin_xmt));
                    left_out.drop_with_or_give_up(
                        (NodeKind::Loop, node),
                        id.clone(),
                        needs,
                        || {
                            unresolved(format!(
                                "LOOP {loop_xmt} states a FIN ring that forms no coedge ring"
                            ))
                        },
                    )?;
                    left_out.release_with_entry((NodeKind::Loop, *loop_xmt), id.clone());
                }
                continue;
            };

            ir.geometry.loops.push(Loop {
                id: id.clone(),
                face: face.clone(),
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
                native_ref: None,
            });
            let key: FaceId = face.clone();

            let loops = match face_loops.entry(key) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
            };

            loops.push(id.clone());
        }
    }
    let mut emitted_faces = BTreeMap::new();
    for pending in pending_faces {
        let boundary_failed = if let Some(failure) = face_loop_failures.remove(&pending.xmt) {
            partial_face_losses.push((pending.id.clone(), crate::loss::NxLossCode::TopologyFaceLoopUnresolved.note(
                format!("parasolid#{stream_index} FACE {} has an unresolved boundary: {failure}; face {} is stated partial",
                pending.xmt, pending.id),
            )));
            true
        } else {
            false
        };
        let mut loops = face_loops.remove(&pending.id).unwrap_or_default();
        let face_node = graph.get(NodeKind::Face, pending.xmt);
        let has_null_loop_ref = face_node
            .and_then(Node::face_fields)
            .is_some_and(|fields| fields.loop_xmt.is_none());
        if loops.is_empty() && !boundary_failed && has_null_loop_ref {
            loops = whole_sphere_pole_loops(
                ir,
                annotations,
                &scope,
                source_stream,
                face_node,
                &pending.id,
                &pending.surface,
            )?;
        }

        let partial = boundary_failed
            || partial_faces.contains(&pending.id)
            || partial_face_xmts.contains(&pending.xmt);
        let loops = if partial {
            cadmpeg_ir::topology::FaceLoops::PartialUnspecified { loops }
        } else {
            let surface = if loops.is_empty() {
                ir.geometry
                    .surfaces
                    .iter()
                    .find(|surface| surface.id == pending.surface)
                    .map(|surface| &surface.geometry)
            } else {
                None
            };
            let (loops, unbounded) =
                cadmpeg_ir::topology::FaceLoops::unspecified_on(loops, surface);
            if let Some(kind) = unbounded {
                partial_face_losses.push((pending.id.clone(), crate::loss::NxLossCode::TopologyFaceBoundaryUnstated.note(
                    format!("parasolid#{stream_index} FACE {} states no loop on a {kind}, which no face covers whole: face {} is stated partial",
                    pending.xmt, pending.id),
                )));
            }
            loops
        };
        emitted_faces.insert(pending.xmt, pending.id.clone());
        ir.geometry.faces.push(Face {
            id: pending.id,
            shell: pending.shell,
            surface: pending.surface,
            sense: pending.sense,
            loops,
            name: None,
            color: None,
            tolerance: pending.tolerance,
            native_ref: None,
        });
    }
    for (face_xmt, note) in vertex_loop_losses {
        if let Some(face) = emitted_faces.get(&face_xmt) {
            partial_face_losses.push((face.clone(), note));
        }
    }
    attach_tolerant_edge_intersections_with_budget(
        ir,
        graph,
        &edges,
        (&scope, source_stream),
        annotations,
        adaptive_geometry_budget,
    )?;
    intersection_index.complete_from_stream(ir, intersection_starts);
    complete_tolerant_intersection_pcurves_from_serialized_branches_for_stream_with_budget(
        ctx,
        ir,
        &serialized_branch_pcurves,
        intersection_starts.coedges,
        intersection_starts.procedural_curves,
        annotations,
        completion_geometry_budget,
    )?;
    complete_exact_boundary_intersection_pcurves_with_budget(
        ir,
        annotations,
        procedural_start,
        exact_transfer_budget,
        completion_geometry_budget,
    )?;
    complete_intersection_pcurves_from_opposite_charts_with_budget(
        ir,
        procedural_start,
        completion_transfer_budget,
        completion_geometry_budget,
    )?;
    intersection_index.complete_new_pcurves_from_stream(ir, intersection_starts.pcurves);

    let mut owned_edges: BTreeSet<EdgeId> = BTreeSet::new();
    for coedge in &ir.geometry.coedges {
        owned_edges.insert(coedge.edge.clone());
    }
    let mut fins_by_edge = BTreeMap::new();
    let mut fins_by_vertex = BTreeMap::new();
    for fin_xmt in &valid_fin_xmts {
        if left_out.get(NodeKind::Fin, *fin_xmt).is_none() {
            continue;
        }
        let fields = graph
            .get(NodeKind::Fin, *fin_xmt)
            .and_then(Node::fin_fields);
        if let Some(edge) = fields.and_then(|fields| fields.edge) {
            fins_by_edge.entry(u32::from(edge)).or_insert(*fin_xmt);
        }
        if let Some(vertex) = fields.and_then(|fields| fields.vertex) {
            fins_by_vertex.entry(u32::from(vertex)).or_insert(*fin_xmt);
        }
    }
    let mut candidate_edges = BTreeSet::new();
    for (edge_xmt, edge) in edges {
        if !owned_edges.contains(&edge) {
            if let Some(dropped) = ir
                .geometry
                .edges
                .iter()
                .find(|candidate| candidate.id == edge)
            {
                for vertex in dropped.vertices() {
                    vertex_needs
                        .entry(vertex.clone())
                        .or_insert((NodeKind::Edge, edge_xmt));
                }
            }
            if let Some(node) = graph.get(NodeKind::Edge, edge_xmt) {
                left_out.drop_with_or_give_up(
                    (NodeKind::Edge, node),
                    edge.clone(),
                    fins_by_edge
                        .get(&edge_xmt)
                        .map(|fin_xmt| (NodeKind::Fin, *fin_xmt)),
                    || unresolved(format!("EDGE {edge_xmt} has no emitted FIN")),
                )?;
            }
        }
        candidate_edges.insert(edge);
    }
    ir.geometry
        .edges
        .retain(|edge| !candidate_edges.contains(&edge.id) || owned_edges.contains(&edge.id));
    let mut retained_vertices: BTreeSet<VertexId> = BTreeSet::new();
    for edge in &ir.geometry.edges {
        for vertex in edge.vertices() {
            retained_vertices.insert(vertex.clone());
        }
    }
    for loop_ in &ir.geometry.loops {
        retained_vertices.extend(loop_.singular_vertex().map(|(vertex, _)| vertex.clone()));
    }
    let scope_prefix = scope.prefix();
    let vertex_xmts: BTreeMap<&VertexId, u32> =
        vertices.iter().map(|(xmt, id)| (id, *xmt)).collect();
    for vertex in &ir.geometry.vertices {
        if !vertex.id.as_str().starts_with(&scope_prefix) || retained_vertices.contains(&vertex.id)
        {
            continue;
        }
        let xmt = vertex_xmts.get(&vertex.id).copied();
        let needs = vertex_needs.get(&vertex.id).copied().or_else(|| {
            xmt.and_then(|xmt| fins_by_vertex.get(&xmt))
                .map(|fin_xmt| (NodeKind::Fin, *fin_xmt))
        });
        match xmt.and_then(|xmt| graph.get(NodeKind::Vertex, xmt)) {
            Some(node) => left_out.drop_with_or_give_up(
                (NodeKind::Vertex, node),
                vertex.id.clone(),
                needs,
                || unresolved(format!("VERTEX {} is on no emitted EDGE", node.xmt())),
            )?,
            None => left_out.drop_built(vertex.id.clone(), needs),
        }
    }
    ir.geometry.vertices.retain(|vertex| {
        !vertex.id.as_str().starts_with(&scope_prefix) || retained_vertices.contains(&vertex.id)
    });
    left_out.finish(topology_losses, given_up);
    Ok(endpoint_witnesses)
}

/// Leaves out the boundary of each FACE whose loop chain does not resolve,
/// none of which emission states. Each LOOP that names such a FACE is given
/// up, or is dropped with the FACE where emission left the FACE out. Each FIN
/// of that LOOP is dropped with it, and each EDGE and VERTEX that such a FIN
/// names, or an EDGE that names such a FIN, is dropped with the FIN when
/// emission has no candidate for it.
fn leave_out_unresolved_boundaries(
    left_out: &mut LeftOut<'_, '_, '_>,
    (graph, scope): (&Graph, &IdScope),
    failures: &BTreeMap<u32, FaceLoopFailure>,
    (edge_candidates, vertex_candidates): (&BTreeSet<u32>, &BTreeSet<u32>),
    unresolved: &impl Fn(String) -> cadmpeg_ir::report::loss::LossNote,
) -> Result<(), CodecError> {
    if failures.is_empty() {
        return Ok(());
    }
    let mut fins_by_loop: BTreeMap<u32, Vec<&Node>> = BTreeMap::new();
    for fin in graph.of_kind(NodeKind::Fin) {
        let loop_xmt = match fin.fin_fields() {
            Some(fields) => fields.loop_xmt,
            None => fin
                .vertex_loop_fin_fields()
                .and_then(|fields| fields.loop_xmt),
        };
        if let Some(loop_xmt) = loop_xmt {
            fins_by_loop
                .entry(u32::from(loop_xmt))
                .or_default()
                .push(fin);
        }
    }
    let mut edges_by_fin: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for edge in graph.of_kind(NodeKind::Edge) {
        if let Some(fin) = edge.edge_fields().and_then(|fields| fields.fin) {
            edges_by_fin
                .entry(u32::from(fin))
                .or_default()
                .push(edge.xmt());
        }
    }
    for node in graph.of_kind(NodeKind::Loop) {
        let Some(face_xmt) = node
            .loop_fields()
            .and_then(|fields| fields.face.map(u32::from))
        else {
            continue;
        };
        let Some(failure) = failures.get(&face_xmt) else {
            continue;
        };
        left_out.drop_with_or_give_up(
            (NodeKind::Loop, node),
            scope.id_from_display::<Identity>(
                &cadmpeg_ir::identity_component!("loop"),
                node.xmt(),
            )?,
            Some((NodeKind::Face, face_xmt)),
            || {
                unresolved(format!(
                    "LOOP {} of FACE {face_xmt} is withheld: the face's loop chain does not \
                     resolve: {failure}",
                    node.xmt()
                ))
            },
        )?;
        let Some(loop_entry) = left_out.get(NodeKind::Loop, node.xmt()).cloned() else {
            continue;
        };
        for fin in fins_by_loop.get(&node.xmt()).into_iter().flatten() {
            let (needs, edges, vertex) = match fin.fin_fields() {
                Some(fields) => {
                    left_out.drop_with(
                        (NodeKind::Fin, fin.xmt()),
                        scope.id_from_display::<Identity>(
                            &cadmpeg_ir::identity_component!("fin"),
                            fin.xmt(),
                        )?,
                        &loop_entry,
                    );
                    let Some(fin_entry) = left_out.get(NodeKind::Fin, fin.xmt()).cloned() else {
                        continue;
                    };
                    let edges = fields
                        .edge
                        .map(u32::from)
                        .into_iter()
                        .chain(edges_by_fin.get(&fin.xmt()).into_iter().flatten().copied())
                        .collect::<Vec<_>>();
                    (fin_entry, edges, fields.vertex)
                }
                None => (
                    loop_entry.clone(),
                    Vec::new(),
                    fin.vertex_loop_fin_fields()
                        .and_then(|fields| fields.vertex),
                ),
            };
            for edge in edges {
                if !edge_candidates.contains(&edge) && graph.get(NodeKind::Edge, edge).is_some() {
                    left_out.drop_with(
                        (NodeKind::Edge, edge),
                        scope.id_from_display::<Identity>(
                            &cadmpeg_ir::identity_component!("edge"),
                            edge,
                        )?,
                        &needs,
                    );
                }
            }
            if let Some(vertex) = vertex.map(u32::from).filter(|vertex| {
                !vertex_candidates.contains(vertex)
                    && graph.get(NodeKind::Vertex, *vertex).is_some()
            }) {
                left_out.drop_with(
                    (NodeKind::Vertex, vertex),
                    scope.id_from_display::<Identity>(
                        &cadmpeg_ir::identity_component!("vertex"),
                        vertex,
                    )?,
                    &needs,
                );
            }
        }
    }
    Ok(())
}

/// The fields, POINT, and position of a VERTEX whose point resolves.
fn resolved_vertex_point<'points>(
    graph: &Graph,
    points: &'points BTreeMap<u32, PointId>,
    point_positions: &BTreeMap<PointId, Point3>,
    vertex_xmt: u32,
) -> Option<(VertexFields, &'points PointId, Point3)> {
    let (fields, point_xmt) = graph.resolved_vertex(vertex_xmt)?;
    let point_ref = points.get(&point_xmt)?;
    let position = point_positions.get(point_ref).copied()?;
    Some((fields, point_ref, position))
}

pub(super) struct UnresolvedTopologyStream<'inputs> {
    pub(super) stream_index: usize,
    pub(super) graph: &'inputs Graph,
    pub(super) surfaces: &'inputs mut BTreeMap<u32, SurfaceId>,
    pub(super) curves: &'inputs mut BTreeMap<u32, CurveId>,
    pub(super) pcurves: &'inputs BTreeMap<u32, PcurveId>,
    pub(super) source_stream: &'inputs cadmpeg_ir::annotations::StreamHandle,
}

pub(super) fn retain_unresolved_topology_carriers(
    ir: &mut CadIr,
    unresolved_topology_stream: UnresolvedTopologyStream<'_>,
    annotations: &mut AnnotationBuilder,
) -> Result<(), CodecError> {
    let UnresolvedTopologyStream {
        stream_index,
        graph,
        surfaces,
        curves,
        pcurves,
        source_stream,
    } = unresolved_topology_stream;

    let scope = IdScope::stream_from_index(stream_index)?;
    let unknown: UnknownId = IdScope::container()
        .id_from_display(&cadmpeg_ir::identity_component!("parasolid"), stream_index)?;
    for face in graph.of_kind(NodeKind::Face) {
        let Some(surface_xmt) = face
            .face_fields()
            .and_then(|fields| fields.surface.map(u32::from))
        else {
            continue;
        };
        if surface_xmt <= 1 || surfaces.contains_key(&surface_xmt) {
            continue;
        }
        let id: SurfaceId = scope.id_from_display(
            &cadmpeg_ir::identity_component!("surface"),
            format_args!("unknown-{surface_xmt}"),
        )?;
        annotations.note(
            &id,
            source_stream,
            cadmpeg_core::decode::u64_from_index(face.pos()),
            Some("UNRESOLVED_SURFACE_REFERENCE"),
        );
        annotations.exactness(&id, Exactness::Unknown);

        ir.geometry.surfaces.push(Surface {
            id: id.clone(),
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown {
                record: Some(unknown.clone()),
            }),
            source_object: None,
            native_ref: None,
        });
        surfaces.insert(surface_xmt, id);
    }

    for edge in graph.of_kind(NodeKind::Edge) {
        let Some(curve_xmt) = edge
            .edge_fields()
            .and_then(|fields| fields.curve.map(u32::from))
        else {
            continue;
        };
        if curve_xmt <= 1 || curves.contains_key(&curve_xmt) || pcurves.contains_key(&curve_xmt) {
            continue;
        }
        let id: CurveId = scope.id_from_display(
            &cadmpeg_ir::identity_component!("curve"),
            format_args!("unknown-{curve_xmt}"),
        )?;
        annotations.note(
            &id,
            source_stream,
            cadmpeg_core::decode::u64_from_index(edge.pos()),
            Some("UNRESOLVED_CURVE_REFERENCE"),
        );
        annotations.exactness(&id, Exactness::Unknown);

        ir.geometry.curves.push(Curve {
            parameter_range: None,
            id: id.clone(),
            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                record: Some(unknown.clone()),
            }),
            source_object: None,
            native_ref: None,
        });
        curves.insert(curve_xmt, id);
    }
    Ok(())
}

pub(super) fn annotate_node(
    annotations: &mut AnnotationBuilder,
    key: impl Into<cadmpeg_ir::annotations::AnnotationKey>,
    stream: &cadmpeg_ir::annotations::StreamHandle,
    node: &Node,
    tag: &str,
) {
    annotations.note(
        key,
        stream,
        cadmpeg_core::decode::u64_from_index(node.pos()),
        Some(tag),
    );
}

pub(super) fn surface_tag(geometry: &SolvedSurfaceGeometry) -> &'static str {
    match geometry {
        SolvedSurfaceGeometry::Plane(_) => "PLANE",
        SolvedSurfaceGeometry::Cylinder(_) => "CYLINDER",
        SolvedSurfaceGeometry::Cone(_) => "CONE",
        SolvedSurfaceGeometry::Sphere(_) => "SPHERE",
        SolvedSurfaceGeometry::Torus(_) => "TORUS",
        SolvedSurfaceGeometry::Nurbs(_) => "B_SPLINE_SURFACE",
        SolvedSurfaceGeometry::Polygonal(_) => "POLYGONAL_SURFACE",
        SolvedSurfaceGeometry::Transformed(placed) => surface_tag(placed.basis()),
        SolvedSurfaceGeometry::Unknown { .. } => "UNKNOWN_SURFACE",
    }
}

pub(super) fn curve_tag(geometry: &SolvedCurveGeometry) -> &'static str {
    match geometry {
        SolvedCurveGeometry::Line(_) => "LINE",
        SolvedCurveGeometry::Circle(_) => "CIRCLE",
        SolvedCurveGeometry::Ellipse(_) => "ELLIPSE",
        SolvedCurveGeometry::Parabola(_) => "PARABOLA",
        SolvedCurveGeometry::Hyperbola(_) => "HYPERBOLA",
        SolvedCurveGeometry::Degenerate(_) => "DEGENERATE_CURVE",
        SolvedCurveGeometry::Nurbs(_) => "B_SPLINE_CURVE",
        SolvedCurveGeometry::Composite { .. } => "COMPOSITE_CURVE",
        SolvedCurveGeometry::Polyline(_) => "POLYLINE",
        SolvedCurveGeometry::Transformed(placed) => curve_tag(placed.basis()),
        SolvedCurveGeometry::Unknown { .. } => "UNKNOWN_CURVE",
    }
}

/// Admit a metre tolerance in millimetres. The scaled value is finite and
/// positive exactly when the metre value is and scaling does not overflow,
/// and the negative missing-tolerance sentinel is refused by its sign.
pub(crate) fn decoded_tolerance(value: f64) -> Option<cadmpeg_ir::scalar::PositiveLength> {
    cadmpeg_ir::scalar::PositiveLength::new(value * 1000.0)
}

struct ClosedEdge<'inputs> {
    scope: &'inputs IdScope,
    edge: &'inputs Node,
    curve: &'inputs CurveId,
    curve_index: usize,
    range: Option<[f64; 2]>,
    source_stream: &'inputs cadmpeg_ir::annotations::StreamHandle,
    tolerance: Option<cadmpeg_ir::scalar::PositiveLength>,
}

fn synthesize_closed_edge_vertex_with_curve_index_and_budget(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    closed_edge: &ClosedEdge<'_>,
    curve_point_cache: &mut CurvePointCache,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<VertexId>, CodecError> {
    let &ClosedEdge {
        scope,
        edge,
        curve,
        curve_index,
        range,
        source_stream,
        tolerance,
    } = closed_edge;

    let parameter = {
        let geometry = &ir.geometry.curves[curve_index].geometry;
        range.map_or_else(
            || match geometry {
                CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
                    nurbs.knots().first().copied().unwrap_or(0.0)
                }
                _ => 0.0,
            },
            |range| range[0],
        )
    };
    let Some(position) = ({
        let geometry = &ir.geometry.curves[curve_index].geometry;
        curve_point_cache.point_with_budget(curve, geometry, parameter, geometry_budget)?
    }) else {
        return Ok(None);
    };
    let point: PointId = scope.id_from_display(
        &cadmpeg_ir::identity_component!("point"),
        format_args!("closed-edge-{}", edge.xmt()),
    )?;
    let vertex: VertexId = scope.id_from_display(
        &cadmpeg_ir::identity_component!("vertex"),
        format_args!("closed-edge-{}", edge.xmt()),
    )?;
    annotations.note(
        &point,
        source_stream,
        cadmpeg_core::decode::u64_from_index(edge.pos()),
        Some("CLOSED_EDGE_POINT"),
    );
    annotations.exactness(&point, Exactness::Inferred);
    annotations.note(
        &vertex,
        source_stream,
        cadmpeg_core::decode::u64_from_index(edge.pos()),
        Some("CLOSED_EDGE_VERTEX"),
    );
    annotations.exactness(&vertex, Exactness::Inferred);

    ir.geometry
        .points
        .push(Point::new(point.clone(), position, None));

    ir.geometry.vertices.push(Vertex {
        id: vertex.clone(),
        point,
        tolerance,
        native_ref: None,
    });
    Ok(Some(vertex))
}

/// Bound a face that covers a whole sphere by a vertex loop at each pole. Parasolid
/// stores such a face with no loop at all, but a neutral face needs a boundary.
fn whole_sphere_pole_loops(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    scope: &IdScope,
    source_stream: &cadmpeg_ir::annotations::StreamHandle,
    node: Option<&Node>,
    face: &FaceId,
    surface: &SurfaceId,
) -> Result<Vec<LoopId>, CodecError> {
    let Some(node) = node else {
        return Ok(Vec::new());
    };
    let Some(sphere) = ir
        .geometry
        .surfaces
        .iter()
        .find(|candidate| &candidate.id == surface)
        .and_then(|candidate| candidate.geometry.solved())
        .and_then(|solved| match solved {
            SolvedSurfaceGeometry::Sphere(sphere) => Some(sphere),
            _ => None,
        })
    else {
        return Ok(Vec::new());
    };
    let center = sphere.center().get();
    let axis = *sphere.frame().axis().as_raw();
    let radius = sphere.radius().get();
    let mut loops = Vec::new();
    for (name, signed) in [("south", -radius), ("north", radius)] {
        let Some(position) = FinitePoint3::new(Point3::new(
            center.x + signed * axis.x,
            center.y + signed * axis.y,
            center.z + signed * axis.z,
        )) else {
            return Ok(Vec::new());
        };
        let label = format!("sphere-pole-{}-{name}", node.xmt());
        let point: PointId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("point"), &label)?;
        let vertex: VertexId =
            scope.id_from_display(&cadmpeg_ir::identity_component!("vertex"), &label)?;
        let id: LoopId = scope.id_from_display(&cadmpeg_ir::identity_component!("loop"), &label)?;
        let offset = cadmpeg_core::decode::u64_from_index(node.pos());
        annotations.note(&point, source_stream, offset, Some("SPHERE_POLE_POINT"));
        annotations.exactness(&point, Exactness::Inferred);
        annotations.note(&vertex, source_stream, offset, Some("SPHERE_POLE_VERTEX"));
        annotations.exactness(&vertex, Exactness::Inferred);
        ir.geometry
            .points
            .push(Point::new(point.clone(), position, None));
        ir.geometry.vertices.push(Vertex {
            id: vertex.clone(),
            point,
            tolerance: None,
            native_ref: None,
        });
        ir.geometry.loops.push(Loop {
            id: id.clone(),
            face: face.clone(),
            boundary: cadmpeg_ir::topology::LoopBoundary::Vertex {
                vertex,
                pcurves: Vec::new(),
            },
            native_ref: None,
        });
        loops.push(id);
    }
    Ok(loops)
}

pub(super) fn canonical_trim_range(geometry: &CurveGeometry, raw: [f64; 2]) -> Option<[f64; 2]> {
    match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_)) => {
            let range = [raw[0] * 1000.0, raw[1] * 1000.0];
            range.into_iter().all(f64::is_finite).then_some(range)
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            let domain = [*nurbs.knots().first()?, *nurbs.knots().last()?];
            let epsilon =
                EPS_EMIT_CANONICAL_TRIM_RANGE_E6 * (1.0 + domain[0].abs().max(domain[1].abs()));
            if raw
                .iter()
                .any(|value| *value < domain[0] - epsilon || *value > domain[1] + epsilon)
            {
                None
            } else {
                Some([
                    raw[0].clamp(domain[0], domain[1]),
                    raw[1].clamp(domain[0], domain[1]),
                ])
            }
        }
        _ => Some(raw),
    }
}

#[cfg(test)]
pub(super) mod test_support;

const MAX_CURVE_POINT_CACHE_ENTRIES: usize = 131_072;

#[derive(Default)]
struct CurvePointCache {
    entries: BTreeMap<CurveId, BTreeMap<u64, Option<FinitePoint3>>>,
    len: usize,
}

impl CurvePointCache {
    fn point_with_budget(
        &mut self,
        curve: &CurveId,
        geometry: &CurveGeometry,
        parameter: f64,
        geometry_budget: &GeometryWorkBudget<'_>,
    ) -> Result<Option<FinitePoint3>, CodecError> {
        let bits = parameter.to_bits();
        if let Some(point) = self.entries.get(curve).and_then(|values| values.get(&bits)) {
            return Ok(*point);
        }
        let point = finite_or_refusal(
            cadmpeg_ir::eval::admission::EvaluationAdmission::Decode(geometry_budget.ctx)
                .within_work_slice(geometry_budget, |admission| {
                    cadmpeg_ir::eval::decode::curve_point(admission, geometry, parameter)
                }),
        )?;
        if self.len < MAX_CURVE_POINT_CACHE_ENTRIES {
            if !self.entries.contains_key(curve) {
                self.entries.insert(curve.clone(), BTreeMap::new());
            }
            if let Some(values) = self.entries.get_mut(curve) {
                values.insert(bits, point);
                self.len += 1;
            }
        }
        Ok(point)
    }
}

/// Sense of a curve carrier's common header relative to its parameterization.
fn curve_sense(graph: &Graph, xmt: u32) -> Option<cadmpeg_ir::topology::Sense> {
    [
        NodeKind::Line,
        NodeKind::Circle,
        NodeKind::Ellipse,
        NodeKind::Intersection,
        NodeKind::BCurve,
        NodeKind::TrimmedCurve,
        NodeKind::SpCurve,
    ]
    .into_iter()
    .find_map(|kind| graph.get(kind, xmt))
    .and_then(Node::common_header)
    .map(|(sense, _)| sense)
}

/// End-pole gap, as a fraction of the polygon's length, below which a control polygon is closed.
const CLOSED_POLYGON_GAP: f64 = 1.0e-6;

/// Whether `start` lies after `end` along an open spline. The control polygon stays close to the
/// spline, and the position of a point along the polygon needs no evaluation of the curve.
fn open_spline_runs_from_end_to_start(
    geometry: &CurveGeometry,
    start: Point3,
    end: Point3,
) -> bool {
    let Some(SolvedCurveGeometry::Nurbs(nurbs)) = geometry.solved() else {
        return false;
    };
    let poles = nurbs.pole_rows().raw_points();
    let (Some(first), Some(last)) = (poles.first(), poles.last()) else {
        return false;
    };
    let length = poles
        .windows(2)
        .map(|pair| pair[0].distance(pair[1]))
        .sum::<f64>();
    // A curve that closes on itself has a seam, so the order of two positions says nothing.
    if first.distance(*last) <= length * CLOSED_POLYGON_GAP {
        return false;
    }
    match (
        polygon_position(&poles, start),
        polygon_position(&poles, end),
    ) {
        (Some(from), Some(to)) => to < from,
        _ => false,
    }
}

/// The distance along a control polygon of the polygon point nearest to `point`.
fn polygon_position(poles: &[Point3], point: Point3) -> Option<f64> {
    let mut walked = 0.0;
    let mut nearest: Option<(f64, f64)> = None;
    for pair in poles.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let span = cadmpeg_ir::math::Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z);
        let arm = cadmpeg_ir::math::Vector3::new(point.x - a.x, point.y - a.y, point.z - a.z);
        let length = span.norm();
        let along = if length > 0.0 {
            (arm.dot(span) / (length * length)).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let foot = Point3::new(
            a.x + span.x * along,
            a.y + span.y * along,
            a.z + span.z * along,
        );
        let distance = foot.distance(point);
        if nearest.is_none_or(|(best, _)| distance < best) {
            nearest = Some((distance, walked + length * along));
        }
        walked += length;
    }
    nearest.map(|(_, position)| position)
}

fn reversed(sense: cadmpeg_ir::topology::Sense) -> cadmpeg_ir::topology::Sense {
    match sense {
        cadmpeg_ir::topology::Sense::Forward => cadmpeg_ir::topology::Sense::Reversed,
        cadmpeg_ir::topology::Sense::Reversed => cadmpeg_ir::topology::Sense::Forward,
    }
}

struct EdgeGeometryRange<'inputs> {
    geometry: &'inputs CurveGeometry,
    curve: &'inputs CurveId,
    range: [f64; 2],
    start_position: Point3,
    start_tolerance: Option<f64>,
    end_position: Point3,
    end_tolerance: Option<f64>,
    edge_tolerance: Option<f64>,
    procedural_curve: bool,
}

fn orient_edge_range_for_geometry_with_budget(
    edge_geometry_range: &EdgeGeometryRange<'_>,
    curve_point_cache: &mut CurvePointCache,
    geometry_budget: &GeometryWorkBudget<'_>,
) -> Result<Option<([f64; 2], bool)>, CodecError> {
    let &EdgeGeometryRange {
        geometry,
        curve,
        range,
        start_position,
        start_tolerance,
        end_position,
        end_tolerance,
        edge_tolerance,
        procedural_curve,
    } = edge_geometry_range;

    let range = if range[0] <= range[1] {
        range
    } else {
        [range[1], range[0]]
    };
    let range = match geometry {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_)) => {
            let sweep = range[1] - range[0];
            if !(0.0..=std::f64::consts::TAU).contains(&sweep) {
                return Ok(None);
            }
            let start = range[0].rem_euclid(std::f64::consts::TAU);
            [start, start + sweep]
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(_)) => {
            let sweep = range[1] - range[0];
            if !(0.0..=std::f64::consts::TAU).contains(&sweep) {
                return Ok(None);
            }
            let start = range[0].rem_euclid(std::f64::consts::TAU);
            [start, start + sweep]
        }
        _ => range,
    };
    let at = match (
        curve_point_cache.point_with_budget(curve, geometry, range[0], geometry_budget)?,
        curve_point_cache.point_with_budget(curve, geometry, range[1], geometry_budget)?,
    ) {
        (Some(start), Some(end)) => [start.get(), end.get()],
        _ if procedural_curve => {
            return Ok(Some((range, false)));
        }
        _ => return Ok(None),
    };
    let allowance = [edge_tolerance, start_tolerance, end_tolerance]
        .into_iter()
        .flatten()
        .fold(0.0_f64, f64::max);
    if Point3::distance(at[0], start_position) <= allowance
        && Point3::distance(at[1], end_position) <= allowance
    {
        Ok(Some((range, false)))
    } else if Point3::distance(at[1], start_position) <= allowance
        && Point3::distance(at[0], end_position) <= allowance
    {
        Ok(Some((range, true)))
    } else {
        Ok(None)
    }
}

pub(super) fn unknown_stream(
    ctx: &DecodeContext<'_>,
    si: usize,
    stream: &Stream,
) -> Result<UnknownRecord, CodecError> {
    ctx.retain_bytes(
        cadmpeg_core::decode::u64_from_index(stream.inflated.len()),
        "retain NX unknown stream",
    )?;
    unknown_stream_record(si, stream, Some(stream.inflated.clone()))
}

pub(super) fn unknown_stream_metadata(
    si: usize,
    stream: &Stream,
) -> Result<UnknownRecord, CodecError> {
    unknown_stream_record(si, stream, None)
}

pub(super) fn retain_unknown_stream_data(
    ctx: &DecodeContext<'_>,
    stream: &Stream,
    unknown: &mut UnknownRecord,
) -> Result<(), CodecError> {
    if unknown.data().is_none() {
        ctx.retain_bytes(
            cadmpeg_core::decode::u64_from_index(stream.inflated.len()),
            "retain NX unknown stream",
        )?;
        unknown.retain_data(stream.inflated.clone());
    }
    Ok(())
}

fn unknown_stream_record(
    si: usize,
    stream: &Stream,
    data: Option<Vec<u8>>,
) -> Result<UnknownRecord, CodecError> {
    let id = format!("{}{si}", crate::RETAINED_PARASOLID_STREAM_ID_PREFIX);
    let id = UnknownId::mint(id).map_err(|error| CodecError::Malformed(error.to_string()))?;
    let offset = cadmpeg_core::decode::u64_from_index(stream.file_offset);
    match data {
        Some(data) => Ok(UnknownRecord::retained(id, offset, data, Vec::new())),
        None => {
            let digest = format!("{}", LowerHex(&sha256(&stream.inflated)));
            Ok(UnknownRecord::unavailable(
                id,
                offset,
                cadmpeg_core::decode::u64_from_index(stream.inflated.len()),
                digest,
                Vec::new(),
            ))
        }
    }
}

/// Builds source metadata from classified layers and the container scan.
pub(super) fn source_meta(
    scan: &Scan,
    dialects: &DialectLayers,
    parsed: Option<&ParsedStreams<'_>>,
) -> Result<SourceMeta, cadmpeg_core::CodecError> {
    let mut attributes = BTreeMap::new();
    for (index, path) in scan
        .container
        .external_reference_paths()
        .into_iter()
        .enumerate()
    {
        insert_source_attribute(
            &mut attributes,
            format_args!("external_reference.{index}"),
            path,
        );
    }
    let mut preview_count = 0usize;
    for entry in scan
        .container
        .entries
        .iter()
        .filter(|entry| entry.name == "/Root/images/preview")
    {
        let Some((offset, size)) = entry.file_span() else {
            continue;
        };
        let (Ok(start), Ok(size)) = (usize::try_from(offset), usize::try_from(size)) else {
            continue;
        };
        let Some(end) = start.checked_add(size) else {
            continue;
        };
        let Some(payload) = scan.container.data.get(start..end) else {
            continue;
        };
        let Some((width, height, precision, components)) = jpeg_dimensions(payload) else {
            continue;
        };
        insert_source_attribute(
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_width"),
            width,
        );
        insert_source_attribute(
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_height"),
            height,
        );
        insert_source_attribute(
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_precision"),
            precision,
        );
        insert_source_attribute(
            &mut attributes,
            format_args!("jpeg_preview_{preview_count}_components"),
            components,
        );
        preview_count += 1;
    }
    for (index, (ordinal, stream)) in scan
        .streams
        .iter()
        .enumerate()
        .filter(|(_, stream)| stream.kind() == StreamKind::Deltas)
        .enumerate()
    {
        let uncached;
        let owner = if let Some(owner) = parsed.and_then(|parsed| parsed.deltas(ordinal)) {
            owner
        } else {
            uncached = cadmpeg_parasolid::records::DeltasRecords::read(&stream.inflated)?;
            &uncached
        };
        insert_source_attribute(
            &mut attributes,
            format_args!("deltas.{index}.selection"),
            "unresolved",
        );
        insert_source_attribute(
            &mut attributes,
            format_args!("deltas.{index}.grammar"),
            "sequential_owned_records",
        );
        if let cadmpeg_parasolid::records::deltas::DeltasEnd::Stopped { reason, .. } = owner.end() {
            insert_source_attribute(
                &mut attributes,
                format_args!("deltas.{index}.stop_reason"),
                format_args!("{reason:?}"),
            );
        }
    }
    Ok(SourceMeta::classified(
        dialects.clone(),
        cadmpeg_core::text::named_entries("the nx part", attributes)?,
    ))
}

fn insert_source_attribute(
    attributes: &mut BTreeMap<String, String>,
    key: impl Display,
    value: impl Display,
) {
    attributes.insert(key.to_string(), value.to_string());
}

#[cfg(test)]
mod tests {
    use super::super::geometry_work::GeometryWorkBudget;
    use super::{source_meta, unknown_stream, unknown_stream_metadata, CurvePointCache};
    use crate::container::Container;
    use crate::decode::Scan;
    use crate::parasolid::Stream;
    use cadmpeg_core::serde_erase::json;
    use cadmpeg_core::CodecError;
    use cadmpeg_ir::geometry::CurveGeometry;
    use cadmpeg_ir::geometry::SolvedCurveGeometry;
    use cadmpeg_ir::ids::CurveId;
    use cadmpeg_ir::math::Point3;

    #[test]
    fn an_edge_whose_vertices_sit_in_reverse_order_on_an_open_spline_runs_against_it() {
        let spline = |poles: Vec<Point3>| {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                    2,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    poles,
                    None,
                    false,
                )
                .unwrap(),
            ))
        };
        let open = spline(vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(5.0, 4.0, 0.0),
            Point3::new(10.0, 0.0, 0.0),
        ]);
        let (near_start, near_end) = (Point3::new(2.0, 1.5, 0.0), Point3::new(8.0, 1.5, 0.0));

        assert!(!super::open_spline_runs_from_end_to_start(
            &open, near_start, near_end
        ));
        assert!(super::open_spline_runs_from_end_to_start(
            &open, near_end, near_start
        ));

        let closed = spline(vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(5.0, 4.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ]);
        assert!(!super::open_spline_runs_from_end_to_start(
            &closed, near_end, near_start
        ));
    }

    fn empty_source_scan() -> Scan<'static> {
        Scan {
            container: Container {
                data: Vec::new().into(),
                layout: crate::container::test_modern_layout(0x06),
                entries: Vec::new(),
                fastload_table: None,
                indexed_section_layouts: std::sync::OnceLock::new(),
                om_section_cache: std::sync::OnceLock::new(),
            },
            streams: Vec::new(),
            untyped: std::cell::RefCell::default(),
        }
    }

    fn preview_stream(bytes: Vec<u8>) -> Stream {
        Stream {
            file_offset: 0,
            consumed: 0,
            compressed: true,
            inflated: bytes,
            body: crate::parasolid::StreamBody::Preview,
        }
    }

    #[test]
    fn unknown_stream_metadata_preserves_identity_and_digest() {
        let stream = preview_stream(vec![7]);

        let unknown = unknown_stream_metadata(0, &stream).unwrap();
        assert_eq!(unknown.id().as_str(), "nx:container:parasolid#0");
        assert_eq!(unknown.offset(), 0);
        assert_eq!(unknown.data(), None);
        let wire = json::to_value(&unknown).unwrap();
        assert_eq!(
            wire["retention"]["sha256"],
            cadmpeg_ir::hash::sha256_hex(&stream.inflated)
        );
    }

    #[test]
    fn source_meta_reuses_the_prepared_delta_census() {
        let mut scan = empty_source_scan();
        scan.streams.push(Stream {
            file_offset: 0,
            consumed: 0,
            compressed: true,
            inflated: Vec::new(),
            body: crate::parasolid::StreamBody::Preview,
        });
        scan.streams.push(Stream {
            file_offset: 0,
            consumed: 0,
            compressed: true,
            inflated: vec![0xaa; 50_000],
            body: crate::parasolid::StreamBody::Parasolid {
                subtype: crate::parasolid::ParasolidSubtype::Deltas,
                schema: None,
            },
        });
        let (dialects, _) = crate::dialect::classify_layers(&scan).into_report_parts();
        let parsed =
            crate::native::substrate::ParsedStreams::parse(&scan).expect("prepared delta census");
        let expected = source_meta(&scan, &dialects, None).expect("uncached source metadata");
        let actual = source_meta(&scan, &dialects, Some(&parsed))
            .expect("source metadata reads the existing census");
        assert_eq!(
            json::to_value(actual).expect("source metadata"),
            json::to_value(expected).expect("source metadata")
        );
    }

    #[test]
    fn unknown_stream_copy_refuses_when_retained_budget_is_exhausted() {
        crate::test_support::with_decode_context_over(
            &[0],
            |policy| {
                policy.limits.max_retained_bytes = 2;
            },
            |ctx| {
                let stream = Stream {
                    file_offset: 0,
                    consumed: 0,
                    compressed: true,
                    inflated: vec![1, 2, 3],
                    body: crate::parasolid::StreamBody::Parasolid {
                        subtype: crate::parasolid::ParasolidSubtype::Partition,
                        schema: None,
                    },
                };

                assert!(matches!(
                    unknown_stream(ctx, 0, &stream),
                    Err(CodecError::ResourceLimit(limit))
                        if limit.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes
                            && limit.operation == "retain NX unknown stream"
                ));
            },
        );
    }

    #[test]
    fn curve_point_cache_reuses_an_exact_parameter_evaluation() {
        crate::test_support::with_decode_context(|geometry_ctx| {
            let curve =
                CurveId::mint("test:model:entity#synthetic:curve").expect("identity grammar");
            let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(1.0, 2.0, 3.0), Point3::new(5.0, 7.0, 9.0)],
                    None,
                    false,
                )
                .expect("valid test curve"),
            ));
            let geometry_budget = GeometryWorkBudget::from_context(geometry_ctx, 1024);
            let mut cache = CurvePointCache::default();

            ({
                let first = cache
                    .point_with_budget(&curve, &geometry, 0.25, &geometry_budget)
                    .expect("evaluation stays within session limits")
                    .expect("NURBS evaluation");
                let remaining_after_first = geometry_budget.remaining();
                let second = cache
                    .point_with_budget(&curve, &geometry, 0.25, &geometry_budget)
                    .expect("evaluation stays within session limits")
                    .expect("cached NURBS evaluation");

                assert_eq!(first, second);
                assert_eq!(geometry_budget.remaining(), remaining_after_first);
            });
        });
    }
}
