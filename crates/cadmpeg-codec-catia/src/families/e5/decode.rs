// SPDX-License-Identifier: Apache-2.0
//! E5-stream decode route: analytic carriers, plane fitting, and topology transfer.

use cadmpeg_core::decode::{u64_from_index, ScopedReservation};

type E5PcurveLiftOutput =
    Result<Option<(PcurveGeometry, [f64; 2], [Point3; 2])>, cadmpeg_core::CodecError>;
type E5CurvePlans<'a> = (
    &'a BTreeMap<u32, IntcurveSupportContext>,
    &'a BTreeMap<u32, (SurfaceId, PcurveGeometry, [f64; 2])>,
);

use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve,
    pcurve::{Pcurve, PcurveGeometry, PcurveNurbs},
    Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext, IntcurveSupportSide,
    ProceduralCurve, ProceduralCurveDefinition, ProceduralSurface, SolvedCurveGeometry,
    SolvedSurfaceGeometry, SupportPcurve, Surface, SurfaceCurveFamily, SurfaceGeometry,
};
use cadmpeg_ir::ids::{
    BodyId, CoedgeId, CurveId, EdgeId, FaceId, LoopId, PcurveId, PointId, ProceduralCurveId,
    ProceduralSurfaceId, RegionId, ShellId, SurfaceId, VertexId,
};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::{
    AnchoredVertexUse, Body, BodyKind, Coedge, Edge, Face, Loop, Point, Region, Sense, Shell,
    Vertex,
};
use cadmpeg_ir::units::{FinitePoint2, FiniteVector, OrthonormalFrame3, UnitVector3};
use cadmpeg_ir::AnnotationBuilder;
use cadmpeg_ir::Exactness;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::assemble::{
    annotate, circle_parameter_range_from_surface_branch, insert_unresolved_carrier_loss,
    link_payload_carriers, neutral_model_is_admissible, ordered_range, preserve_raw_payload,
    quintic_jet_pcurve, rational_pcurve_arc,
};
use crate::container::ContainerScan;
use crate::families::{FamilyEntityAdmission, FamilyOutput};
use crate::loss::CatiaLossCode;
use crate::math::{distance, unit_vector};
use crate::solve::union_find::UnionFind;

const EPS_E5_DECODE_COARSE_GEOMETRY: f64 = 1.0e-6;
const EPS_E5_DECODE_POSITION: f64 = 1.0e-8;
const EPS_E5_DECODE_GEOMETRY: f64 = 1.0e-9;
const EPS_E5_DECODE_DEGENERATE: f64 = 1.0e-10;
const EPS_E5_DECODE_EXACT_GEOMETRY: f64 = 1.0e-12;

const E5_ENDPOINT_MATCH_TOLERANCE: f64 = 2e-3;
const EPS_AXIS_ALIGN: f64 = EPS_E5_DECODE_POSITION;
const E5_ISOPARAMETRIC_DIRECTION_TOLERANCE: f64 = EPS_E5_DECODE_POSITION;
const E5_PARAMETER_RELATIVE_TOLERANCE: f64 = EPS_E5_DECODE_GEOMETRY;
const E5_CARRIER_AXIS_COSINE_TOLERANCE: f64 = EPS_E5_DECODE_GEOMETRY;

#[derive(Clone, Copy)]
enum E5IsoparametricDirection {
    ConstantU,
    ConstantV,
}

fn e5_isoparametric_direction(direction: Point2) -> Option<E5IsoparametricDirection> {
    let scale = direction.u.abs().max(direction.v.abs());
    if !scale.is_finite() || scale == 0.0 {
        return None;
    }
    let normalized = Point2::new(direction.u / scale, direction.v / scale);
    match (
        normalized.u.abs() <= E5_ISOPARAMETRIC_DIRECTION_TOLERANCE,
        normalized.v.abs() <= E5_ISOPARAMETRIC_DIRECTION_TOLERANCE,
    ) {
        (true, false) => Some(E5IsoparametricDirection::ConstantU),
        (false, true) => Some(E5IsoparametricDirection::ConstantV),
        (true, true) | (false, false) => None,
    }
}

/// Decode direct E5 circle carriers.  Their edge and face references are a
/// separate record layer, so curves remain unattached until that layer is
/// decoded rather than being assigned speculatively.
pub(in crate::families) fn try_decode_e5(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scan: &ContainerScan,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<FamilyOutput>, cadmpeg_core::CodecError> {
    let Some(stream_range) = scan.e5_record_range.clone() else {
        return Ok(None);
    };
    let stream = &scan.data[stream_range];
    let mut geometry_storage = ctx.reserve_scoped(0, "catia_e5_source_geometry")?;
    let circles = geometry_storage.with_storage(|| crate::families::e5::records::e5_circles(ctx, stream))?;
    let mut surfaces = geometry_storage.with_storage(|| crate::families::e5::records::e5_surfaces(ctx, stream, refusal))?;
    let rolling_ball_jets = geometry_storage.with_storage(|| crate::families::e5::records::e5_rolling_ball_jets(ctx, stream))?;
    let topology = geometry_storage.with_storage(|| crate::families::e5::graph::parse_topology(ctx, stream))?;
    // Edge records and derived vertex candidates are scratch.
    let mut scratch = ctx.reserve_scoped(0, "catia_e5_decode_scratch")?;
    let vertex_count = if let Some(topology) = topology.as_ref() {
        topology.vertex_refs.len()
    } else {
        let edges = scratch.with_storage(|| crate::families::e5::records::e5_edges(ctx, stream))?;
        let mut vertices = HashSet::new();
        for edge in ctx.admit_iter(&edges, "catia_e5_edge_vertex_scan")? {
            for vertex in [edge.start_vertex_id, edge.end_vertex_id] {
                scratch.with_storage(|| {
                    ctx.insert_hash_set(&mut vertices, vertex, "catia_e5_edge_vertex_ids")
                })?;
            }
        }
        vertices.len()
    };
    let roster = geometry_storage.with_storage(|| crate::families::e5::records::e5_vertices(ctx, &scan.data, vertex_count))?;
    let points = if roster.len() == vertex_count {
        roster
    } else if let Some(topology) = topology.as_ref() {
        match scratch.with_storage(|| derive_e5_vertices(ctx, topology, &surfaces, refusal))? {
            Some(derived) => {
                let mut points = geometry_storage.with_storage(|| ctx.vector_storage(derived.len(), "catia_e5_derived_points"))?;
                for point in ctx
                    .admit_iter(&derived, "catia_e5_derived_vertex_scan")?
                    .copied()
                {
                    // A derived vertex that is not finite states no point,
                    // so the decode refuses it where the vertex list is admitted.
                    let Some(point) = FinitePoint3::new(point) else {
                        return Ok(None);
                    };
                    geometry_storage.with_storage(|| ctx.push_vec(&mut points, point, "catia_e5_derived_points"))?;
                }
                points
            }
            None => Vec::new(),
        }
    } else {
        Vec::new()
    };
    drop(scratch);
    if let Some(topology) = &topology {
        geometry_storage.with_storage(|| append_e5_planes(ctx, stream, topology, &points, &mut surfaces))?;
    }
    if circles.is_empty()
        && surfaces.is_empty()
        && rolling_ball_jets.is_empty()
        && points.is_empty()
    {
        return Ok(None);
    }
    let mut ir = CadIr::empty();
    let mut admission = FamilyEntityAdmission::new(ctx);
    let mut annotations = AnnotationBuilder::new();
    let mut unknowns = Vec::new();
    let payload_id = ctx
        .copy_retained_text("catia:payload:unknown#e5", "catia_e5_payload_id")
        .and_then(|text| {
            cadmpeg_ir::ids::UnknownId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
        })?;
    let payload_index =
        preserve_raw_payload(ctx, &mut unknowns, &mut annotations, scan, payload_id)?;
    for (index, point) in ctx
        .admit_iter(&points, "catia_e5_emitted_point_scan")?
        .enumerate()
    {
        let point_id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "pt"),
            index,
            PointId::mint,
            "catia_e5_point_id",
        )?;
        annotate(
            ctx,
            &mut annotations,
            &point_id,
            "e5_0d_03",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        )?;
        admission.reserve_entity(&mut ir.model.points, "catia_e5_model_points")?;
        ir.model.points.push(Point::new(
            point_id.try_clone_for_decode(ctx, "catia_e5_point_record_id")?,
            *point,
            None,
        ));
        let vertex_id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "v"),
            index,
            VertexId::mint,
            "catia_e5_vertex_id",
        )?;
        annotate(
            ctx,
            &mut annotations,
            &vertex_id,
            "MainDataStream+SurfacicReps",
            0,
            "vertex_05_08_01",
            Exactness::ByteExact,
        )?;
        crate::resource::derived_annotation(
            ctx,
            &mut annotations,
            &vertex_id,
            "point",
            "catia_annotation_field",
        )?;
        admission.reserve_entity(&mut ir.model.vertices, "catia_e5_model_vertices")?;
        ir.model.vertices.push(Vertex {
            id: vertex_id,
            point: point_id,
            tolerance: None,
        });
    }
    for (index, circle) in ctx
        .admit_iter(&circles, "catia_e5_emitted_circle_scan")?
        .enumerate()
    {
        let id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "curve"),
            index,
            CurveId::mint,
            "catia_e5_curve_id",
        )?;
        annotate(
            ctx,
            &mut annotations,
            &id,
            "e5_0d_03",
            u64_from_index(circle.pos),
            "circle_carrier",
            Exactness::ByteExact,
        )?;
        admission.reserve_entity(&mut ir.model.curves, "catia_e5_model_curves")?;
        ir.model.curves.push(Curve {
            id,
            geometry: circle
                .geometry
                .try_clone_for_decode(ctx, "catia_e5_model_curve_geometry")?,
            source_object: None,
        });
    }
    for (index, surface) in ctx
        .admit_iter(&surfaces, "catia_e5_emitted_surface_scan")?
        .enumerate()
    {
        let id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "surf"),
            index,
            SurfaceId::mint,
            "catia_e5_surface_id",
        )?;
        annotate(
            ctx,
            &mut annotations,
            &id,
            "e5_0d_03",
            u64_from_index(surface.pos),
            "analytic_surface",
            if matches!(
                surface.geometry,
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
            ) {
                Exactness::Derived
            } else {
                Exactness::ByteExact
            },
        )?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_e5_model_surfaces")?;
        ir.model.surfaces.push(Surface {
            id,
            geometry: surface
                .geometry
                .try_clone_for_decode(ctx, "catia_e5_model_surface_geometry")?,
            source_object: None,
        });
    }
    for (index, jet) in ctx
        .admit_iter(&rolling_ball_jets, "catia_e5_emitted_rolling_ball_jet_scan")?
        .enumerate()
    {
        let surface_index = surfaces.len() + index;
        let surface_id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "surf"),
            surface_index,
            SurfaceId::mint,
            "catia_e5_jet_surface_id",
        )?;
        let procedural_id = crate::resource::compose_index_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "procedural-surf"),
            surface_index,
            ProceduralSurfaceId::mint,
            "catia_e5_jet_procedural_id",
        )?;
        annotate(
            ctx,
            &mut annotations,
            &surface_id,
            "e5_0d_03",
            u64_from_index(jet.pos),
            "rolling_ball_jet_carrier",
            Exactness::ByteExact,
        )?;
        crate::resource::derived_annotation(
            ctx,
            &mut annotations,
            &surface_id,
            "geometry",
            "catia_annotation_field",
        )?;
        admission.reserve_entity(&mut ir.model.surfaces, "catia_e5_model_surfaces")?;
        ir.model.surfaces.push(Surface {
            id: surface_id.try_clone_for_decode(ctx, "catia_e5_jet_surface_record_id")?,
            geometry: SurfaceGeometry::Procedural {
                construction: procedural_id
                    .try_clone_for_decode(ctx, "catia_e5_jet_construction_id")?,
                cache: None,
            },
            source_object: None,
        });
        annotate(
            ctx,
            &mut annotations,
            &procedural_id,
            "e5_0d_03",
            u64_from_index(jet.pos),
            "rolling_ball_jet_definition",
            Exactness::ByteExact,
        )?;
        crate::resource::derived_annotation(
            ctx,
            &mut annotations,
            &procedural_id,
            "surface",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            &mut annotations,
            &procedural_id,
            "definition",
            "catia_annotation_field",
        )?;
        admission.reserve_entity(
            &mut ir.model.procedural_surfaces,
            "catia_e5_model_procedural_surfaces",
        )?;
        let Some(definition) = jet.definition(ctx)? else {
            return Ok(None);
        };
        ir.model
            .procedural_surfaces
            .push(ProceduralSurface::new(procedural_id, definition, None));
    }
    let mut topology_ir = std::mem::replace(&mut ir, CadIr::empty());
    let mut topology_annotations =
        annotations.copy_transaction(ctx, "catia_e5_topology_annotations")?;
    let mut original_curves = Vec::new();
    let mut unused_surfaces = Vec::new();
    let topology_transferred = if let Some(topology) = topology.as_ref() {
        original_curves = std::mem::take(&mut topology_ir.model.curves);
        for curve in ctx.admit_iter(&original_curves, "catia_e5_original_curve_scan")? {
            topology_annotations.remove_entity(ctx, curve.id.as_str())?;
        }
        transfer_e5_topology(
            ctx,
            (&mut topology_ir, &mut topology_annotations),
            topology,
            &surfaces,
            refusal,
            &mut admission,
            &mut unused_surfaces,
        )? && neutral_model_is_admissible(ctx, &mut topology_ir, &unknowns)?
    } else {
        false
    };
    if topology_transferred {
        ir = topology_ir;
        annotations = topology_annotations.into_retained()?;
    } else {
        if topology.is_some() {
            topology_ir.model.bodies.clear();
            topology_ir.model.regions.clear();
            topology_ir.model.shells.clear();
            topology_ir.model.faces.clear();
            topology_ir.model.loops.clear();
            topology_ir.model.coedges.clear();
            topology_ir.model.edges.clear();
            topology_ir.model.pcurves.clear();
            topology_ir.model.procedural_curves.clear();
            topology_ir.model.curves = original_curves;
            ctx.append_vec(
                &mut topology_ir.model.surfaces,
                &mut unused_surfaces,
                "catia_e5_rollback_surfaces",
            )?;
            ctx.sort_unstable_by(
                &mut topology_ir.model.surfaces,
                |value| value.id.as_str(),
                |left, right| e5_source_ordinal(left).cmp(&e5_source_ordinal(right)),
                "catia_e5_rollback_surfaces_sort",
            )?;
            ctx.sort_unstable_by(
                &mut topology_ir.model.points,
                |value| value.id.as_str(),
                |left, right| e5_source_ordinal(left).cmp(&e5_source_ordinal(right)),
                "catia_e5_rollback_points_sort",
            )?;
            ctx.sort_unstable_by(
                &mut topology_ir.model.vertices,
                |value| value.id.as_str(),
                |left, right| e5_source_ordinal(left).cmp(&e5_source_ordinal(right)),
                "catia_e5_rollback_vertices_sort",
            )?;
        }
        ir = topology_ir;
        if !ir.model.vertices.is_empty() {
            attach_e5_free_vertices(ctx, &mut ir, &mut annotations, &mut admission)?;
        }
    }
    let mut losses = Vec::new();
    let (loss_code, message) = if topology_transferred {
        let message = if topology
            .as_ref()
            .is_some_and(|topology| topology.bodies.is_empty())
        {
            "The E5 reference graph is closed; body ownership and shell orientation use an incidence-derived gauge because the stream has no class-0x01 body root."
        } else {
            "The E5 reference graph is closed; face and loop orientation transfer, but body/shell orientation uses an incidence-derived gauge because the root's two trailing orientation signs remain unresolved."
        };
        (CatiaLossCode::TopologyE5GaugeSubstituted, message)
    } else {
        (
            CatiaLossCode::TopologyE5GraphUnclosed,
            "E5 carriers were decoded, but the reference graph could not be transferred with a closed surface/pcurve/vertex binding.",
        )
    };
    crate::resource::push_loss(
        ctx,
        &mut losses,
        loss_code,
        format_args!("{message}"),
        "catia_e5_topology_loss",
    )?;
    insert_unresolved_carrier_loss(ctx, &ir, &mut losses)?;
    link_payload_carriers(ctx, &ir, &mut unknowns[payload_index], &mut annotations)?;
    let annotations = annotations.build();
    Ok(Some(FamilyOutput {
        ir,
        report: DecodeBody {
            transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
            coverage: cadmpeg_ir::report::decode::Coverage::default(),
            losses,
            notes: Vec::new(),
            transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
        },
        annotations,
        unknowns,
        admitted_model_entities: admission.admitted(),
    }))
}

fn e5_source_ordinal(id: &str) -> (bool, usize) {
    match id
        .rsplit_once('#')
        .and_then(|(_, ordinal)| ordinal.parse().ok())
    {
        Some(ordinal) => (false, ordinal),
        None => (true, 0),
    }
}

/// Derives each referenced vertex from the endpoints of the pcurves that use
/// it, when every use agrees. The caller holds the result as scratch.
fn derive_e5_vertices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::families::e5::graph::E5Topology,
    surfaces: &[crate::families::e5::records::E5Surface],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<Vec<Point3>>, cadmpeg_core::CodecError> {
    let mut surface_for_ref = HashMap::new();
    for surface in ctx.admit_iter(surfaces, "catia_e5_derived_surface_scan")? {
        ctx.insert_hash_map(
            &mut surface_for_ref,
            surface.record_id,
            surface,
            "catia_e5_derived_surface_refs",
        )?;
    }
    let mut candidates = HashMap::<u32, Vec<Point3>>::new();
    for face in ctx.admit_iter(&topology.faces, "catia_e5_derived_face_scan")? {
        for loop_ in ctx.admit_iter(&face.loops, "catia_e5_derived_loop_scan")? {
            for member in ctx.admit_iter(&loop_.members, "catia_e5_derived_member_scan")? {
                let pcurve_ref = member.pcurve;
                let edge_ref = member.edge_use;
                let (Some(edge), Some(pcurve)) = (
                    ctx.get_btree_map(&topology.edges, &edge_ref, "catia_e5_derived_edge_lookup")?,
                    ctx.get_btree_map(
                        &topology.pcurves,
                        &pcurve_ref,
                        "catia_e5_derived_pcurve_lookup",
                    )?,
                ) else {
                    return Ok(None);
                };
                let Some(surface) = ctx.get_hash_map(
                    &surface_for_ref,
                    &pcurve.surface_record_id(),
                    "catia_e5_derived_surface_refs",
                )?
                else {
                    return Ok(None);
                };
                let Some((_, range, endpoints)) =
                    e5_pcurve_on_surface(ctx, pcurve, surface, refusal)?
                else {
                    return Ok(None);
                };
                let Some(reversed) =
                    e5_stored_pcurve_reversed(ctx, topology, edge_ref, pcurve_ref, range)?
                else {
                    return Ok(None);
                };
                let endpoints = if reversed {
                    [endpoints[1], endpoints[0]]
                } else {
                    endpoints
                };
                for (vertex, point) in [
                    (edge.start_vertex, endpoints[0]),
                    (edge.end_vertex, endpoints[1]),
                ] {
                    ctx.push_hash_group(
                        &mut candidates,
                        vertex,
                        point,
                        "catia_e5_derived_candidate_keys",
                        "catia_e5_derived_candidate_points",
                    )?;
                }
            }
        }
    }
    let mut points = ctx.vector_storage(topology.vertex_refs.len(), "catia_e5_derived_vertices")?;
    for vertex in ctx.admit_iter(&topology.vertex_refs, "catia_e5_derived_vertex_ref_scan")? {
        let Some(values) =
            ctx.get_hash_map(&candidates, vertex, "catia_e5_derived_candidate_keys")?
        else {
            return Ok(None);
        };
        let Some(point) = values.first().copied() else {
            return Ok(None);
        };
        if ctx.any_by(
            values,
            |candidate| Ok(candidate.distance(point) > E5_ENDPOINT_MATCH_TOLERANCE),
            "catia_e5_derived_candidate_agreement_scan",
        )? {
            return Ok(None);
        }
        ctx.push_vec(&mut points, point, "catia_e5_derived_vertices")?;
    }
    Ok(Some(points))
}

fn append_e5_planes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    stream: &[u8],
    topology: &crate::families::e5::graph::E5Topology,
    points: &[FinitePoint3],
    surfaces: &mut Vec<crate::families::e5::records::E5Surface>,
) -> Result<(), cadmpeg_core::CodecError> {
    // The carrier axes, the plane records, the faces grouped by surface and
    // each frame solve are scratch; only the admitted plane surfaces are kept.
    let mut scratch = ctx.reserve_scoped(0, "catia_e5_plane_scratch")?;
    let mut carrier_axes = HashMap::new();
    for surface in ctx.admit_iter(&*surfaces, "catia_e5_carrier_surface_scan")? {
        let axis = match surface.geometry {
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                *cylinder_surface.frame().axis().as_raw()
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                *cone_surface.frame().axis().as_raw()
            }
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                *torus_surface.frame().axis().as_raw()
            }
            _ => continue,
        };
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut carrier_axes,
                surface.record_id,
                axis,
                "catia_e5_carrier_axes",
            )
        })?;
    }
    let mut faces_by_surface = HashMap::<u32, Vec<&crate::families::e5::graph::E5Face>>::new();
    for face in ctx.admit_iter(&topology.faces, "catia_e5_plane_face_group_scan")? {
        scratch.with_storage(|| {
            ctx.push_hash_group(
                &mut faces_by_surface,
                face.surface,
                face,
                "catia_e5_plane_face_group_keys",
                "catia_e5_plane_face_groups",
            )
        })?;
    }
    let planes = scratch.with_storage(|| crate::families::e5::records::e5_planes(ctx, stream))?;
    for plane in ctx.admit_iter(&planes, "catia_e5_plane_record_scan")? {
        let Some(faces) = ctx.get_hash_map(
            &faces_by_surface,
            &plane.record_id,
            "catia_e5_plane_face_group_keys",
        )?
        else {
            continue;
        };
        let mut normal: Option<Vector3> = None;
        let mut consistent = true;
        for face in ctx.admit_iter(faces, "catia_e5_plane_face_scan")? {
            for loop_ in ctx.admit_iter(&face.loops, "catia_e5_plane_loop_scan")? {
                for member in ctx.admit_iter(&loop_.members, "catia_e5_plane_member_scan")? {
                    let Some(edge) = ctx.get_btree_map(
                        &topology.edges,
                        &member.edge_use,
                        "catia_e5_plane_edge_lookup",
                    )?
                    else {
                        consistent = false;
                        continue;
                    };
                    let Some(support) = ctx.get_btree_map(
                        &topology.curve_supports,
                        &edge.support,
                        "catia_e5_plane_support_lookup",
                    )?
                    else {
                        consistent = false;
                        continue;
                    };
                    for pcurve_ref in
                        ctx.admit_iter(support.pcurves(), "catia_e5_plane_support_pcurve_scan")?
                    {
                        let Some(crate::families::e5::graph::E5Pcurve::Line {
                            surface,
                            direction,
                            ..
                        }) = ctx.get_btree_map(
                            &topology.pcurves,
                            pcurve_ref,
                            "catia_e5_plane_pcurve_lookup",
                        )?
                        else {
                            continue;
                        };
                        if direction[0].get().abs() <= EPS_E5_DECODE_GEOMETRY
                            || direction[1].get().abs() > EPS_E5_DECODE_GEOMETRY
                        {
                            continue;
                        }
                        let Some(&candidate) =
                            ctx.get_hash_map(&carrier_axes, surface, "catia_e5_carrier_axes")?
                        else {
                            continue;
                        };
                        let candidate = canonical_direction(candidate);
                        if normal.is_some_and(|value| {
                            value.x * candidate.x + value.y * candidate.y + value.z * candidate.z
                                < 1.0 - EPS_E5_DECODE_DEGENERATE
                        }) {
                            consistent = false;
                        } else {
                            normal = Some(candidate);
                        }
                    }
                }
            }
        }
        let expected_normal = normal.filter(|_| consistent);
        let Some((normal, u_axis, uv_scale)) = scratch.with_storage(|| {
            solve_e5_plane_frame(ctx, plane.origin, faces, topology, points, expected_normal)
        })?
        else {
            continue;
        };
        let Some(frame) = OrthonormalFrame3::from_units(normal, u_axis) else {
            continue;
        };
        let payload = cadmpeg_ir::geometry::analytic::PlaneSurface::new(plane.origin, frame);
        ctx.push_vec(
            surfaces,
            crate::families::e5::records::E5Surface {
                pos: plane.pos,
                record_id: plane.record_id,
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(payload)),
                uv_scale,
            },
            "catia_e5_plane_surfaces",
        )?;
    }
    Ok(())
}

/// Classify two UV vectors by a scale-normalized determinant.
///
/// A tiny transverse component is common after a circular or spline boundary
/// is serialized as a line-like endpoint tape. Treating that roundoff as a
/// second rank would send plane fitting through an ill-conditioned 2D solve;
/// the known geometric normal must select the rank-one path instead.
fn e5_uv_vectors_are_independent(left: FiniteVector<2>, right: FiniteVector<2>) -> bool {
    const RANK_TOLERANCE: f64 = EPS_E5_DECODE_EXACT_GEOMETRY;
    let scale = left
        .into_iter()
        .chain(right)
        .map(f64::abs)
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return false;
    }
    let left = [left[0] / scale, left[1] / scale];
    let right = [right[0] / scale, right[1] / scale];
    (left[0] * right[1] - left[1] * right[0]).abs() > RANK_TOLERANCE
}

/// The point of a referenced vertex. `vertex_refs` is sorted and unique and
/// `points` holds one point per reference in the same order.
fn e5_vertex_point(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    vertex_refs: &[u32],
    points: &[FinitePoint3],
    vertex: u32,
) -> Result<Option<FinitePoint3>, cadmpeg_core::CodecError> {
    Ok(ctx
        .binary_search(vertex_refs, &vertex, "catia_e5_vertex_ref_search")?
        .ok()
        .and_then(|index| points.get(index).copied()))
}

/// Fits the frame of a plane from the faces that lie on it. The caller holds
/// every intermediate list as scratch.
fn solve_e5_plane_frame(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    origin: FinitePoint3,
    faces: &[&crate::families::e5::graph::E5Face],
    topology: &crate::families::e5::graph::E5Topology,
    points: &[FinitePoint3],
    expected_normal: Option<Vector3>,
) -> Result<Option<(UnitVector3, UnitVector3, [FiniteReal; 2])>, cadmpeg_core::CodecError> {
    if topology.vertex_refs.len() != points.len() {
        return Ok(None);
    }
    let mut segments = Vec::new();
    for face in ctx.admit_iter(faces, "catia_e5_plane_frame_face_scan")? {
        for loop_ in ctx.admit_iter(&face.loops, "catia_e5_plane_frame_loop_scan")? {
            for member in ctx.admit_iter(&loop_.members, "catia_e5_plane_frame_member_scan")? {
                let (Some(edge), Some(pcurve)) = (
                    ctx.get_btree_map(
                        &topology.edges,
                        &member.edge_use,
                        "catia_e5_plane_edge_lookup",
                    )?,
                    ctx.get_btree_map(
                        &topology.pcurves,
                        &member.pcurve,
                        "catia_e5_plane_pcurve_lookup",
                    )?,
                ) else {
                    return Ok(None);
                };
                let Some(uv) = e5_native_uv_endpoints(ctx, pcurve)? else {
                    return Ok(None);
                };
                let (Some(start), Some(end)) = (
                    e5_vertex_point(ctx, &topology.vertex_refs, points, edge.start_vertex)?,
                    e5_vertex_point(ctx, &topology.vertex_refs, points, edge.end_vertex)?,
                ) else {
                    return Ok(None);
                };
                ctx.push_vec(&mut segments, (uv, [start, end]), "catia_e5_plane_segments")?;
            }
        }
    }
    if segments.is_empty() {
        return Ok(None);
    }

    let anchors = 'find_anchors: {
        let mut basis = None;
        for (index, (uv, _)) in ctx
            .admit_iter(&segments, "catia_e5_plane_anchor_segment_scan")?
            .enumerate()
        {
            for endpoint in uv {
                if let Some((basis_index, basis_uv)) = basis {
                    if e5_uv_vectors_are_independent(basis_uv, *endpoint) {
                        break 'find_anchors Some((
                            [basis_index, index],
                            if basis_index == index { 1 } else { 2 },
                        ));
                    }
                } else if endpoint[0] != 0.0 || endpoint[1] != 0.0 {
                    basis = Some((index, *endpoint));
                }
            }
        }
        None
    };
    let endpoint_pairs = |segment: &([FiniteVector<2>; 2], [FinitePoint3; 2]), reversed: bool| {
        let points = if reversed {
            [segment.1[1], segment.1[0]]
        } else {
            segment.1
        };
        [(segment.0[0], points[0]), (segment.0[1], points[1])]
    };
    let endpoint_error = |axes: (Vector3, Vector3),
                          segment: &([FiniteVector<2>; 2], [FinitePoint3; 2]),
                          reversed: bool|
     -> Result<f64, cadmpeg_core::CodecError> {
        plane_frame_residual(
            ctx,
            origin.get().into(),
            &endpoint_pairs(segment, reversed),
            axes.0,
            axes.1,
        )
    };

    let mut fitted_axes = Vec::new();
    if let Some((anchor_indices, anchor_count)) = anchors {
        let anchors = &anchor_indices[..anchor_count];
        for mask in 0usize..(1usize << anchors.len()) {
            let mut orientations =
                ctx.alloc_filled(segments.len(), false, "catia_e5_plane_orientations")?;
            for (bit, &index) in ctx
                .admit_iter(anchors, "catia_e5_plane_anchor_index_scan")?
                .enumerate()
            {
                orientations[index] = mask & (1 << bit) != 0;
            }
            let mut seed_pairs = Vec::new();
            let Some(seed_count) = anchors.len().checked_mul(2) else {
                return Ok(None);
            };
            ctx.reserve_vec(&mut seed_pairs, seed_count, "catia_e5_plane_seed_pairs")?;
            for &index in ctx.admit_iter(anchors, "catia_e5_plane_anchor_pair_scan")? {
                seed_pairs.extend(endpoint_pairs(&segments[index], orientations[index]));
            }
            let Some((seed_u, seed_v, _)) = fit_e5_plane_axes(ctx, origin, &seed_pairs)? else {
                continue;
            };
            for (index, segment) in ctx
                .admit_iter(&segments, "catia_e5_plane_orientation_segment_scan")?
                .enumerate()
            {
                if anchor_indices[..anchor_count]
                    .iter()
                    .any(|anchor| *anchor == index)
                {
                    continue;
                }
                orientations[index] = endpoint_error((seed_u, seed_v), segment, true)?
                    < endpoint_error((seed_u, seed_v), segment, false)?;
            }
            let mut pairs = Vec::new();
            let Some(pair_count) = segments.len().checked_mul(2) else {
                return Ok(None);
            };
            ctx.reserve_vec(&mut pairs, pair_count, "catia_e5_plane_pairs")?;
            for (segment, &reversed) in ctx
                .admit_iter(&segments, "catia_e5_plane_pair_segment_scan")?
                .zip(ctx.admit_iter(&orientations, "catia_e5_plane_orientation_scan")?)
            {
                pairs.extend(endpoint_pairs(segment, reversed));
            }
            if let Some(fit) = fit_e5_plane_axes(ctx, origin, &pairs)? {
                ctx.push_vec(&mut fitted_axes, (fit, pairs), "catia_e5_plane_fits")?;
            }
        }
    } else {
        let Some(normal) = expected_normal else {
            return Ok(None);
        };
        let Some(seed_index) = ctx.position_by(
            &segments,
            |(uv, _)| Ok(uv.iter().any(|point| point[0] != 0.0 || point[1] != 0.0)),
            "catia_e5_plane_rank_one_seed_scan",
        )?
        else {
            return Ok(None);
        };
        for seed_reversed in [false, true] {
            let seed_pairs = endpoint_pairs(&segments[seed_index], seed_reversed);
            let Some((seed_u, seed_v, _)) =
                fit_rank_one_e5_plane_axes(ctx, origin, &seed_pairs, normal)?
            else {
                continue;
            };
            let mut orientations =
                ctx.alloc_filled(segments.len(), false, "catia_e5_plane_orientations")?;
            orientations[seed_index] = seed_reversed;
            for (index, segment) in ctx
                .admit_iter(&segments, "catia_e5_plane_orientation_segment_scan")?
                .enumerate()
            {
                if index == seed_index {
                    continue;
                }
                orientations[index] = endpoint_error((seed_u, seed_v), segment, true)?
                    < endpoint_error((seed_u, seed_v), segment, false)?;
            }
            let mut pairs = Vec::new();
            let Some(pair_count) = segments.len().checked_mul(2) else {
                return Ok(None);
            };
            ctx.reserve_vec(&mut pairs, pair_count, "catia_e5_plane_pairs")?;
            for (segment, &reversed) in ctx
                .admit_iter(&segments, "catia_e5_plane_pair_segment_scan")?
                .zip(ctx.admit_iter(&orientations, "catia_e5_plane_orientation_scan")?)
            {
                pairs.extend(endpoint_pairs(segment, reversed));
            }
            if let Some(fit) = fit_rank_one_e5_plane_axes(ctx, origin, &pairs, normal)? {
                ctx.push_vec(&mut fitted_axes, (fit, pairs), "catia_e5_plane_fits")?;
            }
        }
    }

    let mut candidates: Vec<(UnitVector3, UnitVector3)> = Vec::new();
    for ((u_axis, v_axis, residual), pairs) in
        ctx.admit_iter(&fitted_axes, "catia_e5_plane_fit_scan")?
    {
        let Some(u_axis) = unit_vector(*u_axis) else {
            continue;
        };
        let Some(v_axis) = unit_vector(*v_axis) else {
            continue;
        };
        let orthogonality = u_axis.as_raw().dot(*v_axis.as_raw());
        if !residual.is_finite()
            || !orthogonality.is_finite()
            || *residual > 2e-3
            || orthogonality.abs() > EPS_E5_DECODE_COARSE_GEOMETRY
        {
            continue;
        }
        let (u, v) = (u_axis.as_raw(), v_axis.as_raw());
        let Some(normal) = unit_vector(Vector3::new(
            u.y * v.z - u.z * v.y,
            u.z * v.x - u.x * v.z,
            u.x * v.y - u.y * v.x,
        )) else {
            continue;
        };
        // The returned chart uses unit axes and derives v from normal x u.
        // Validate that chart, not the unrestricted least-squares fit.
        let residual = plane_frame_residual(
            ctx,
            origin.get().into(),
            pairs,
            *u_axis.as_raw(),
            normal.as_raw().cross(*u_axis.as_raw()),
        )?;
        if !residual.is_finite() || residual > E5_ENDPOINT_MATCH_TOLERANCE {
            continue;
        }
        if expected_normal.is_some_and(|expected| {
            let alignment = normal.as_raw().dot(expected);
            !alignment.is_finite() || alignment.abs() < 1.0 - EPS_E5_DECODE_COARSE_GEOMETRY
        }) {
            continue;
        }
        if !ctx.any_by(
            &candidates,
            |(existing_normal, existing_u)| {
                Ok(
                    existing_normal.as_raw().dot(*normal.as_raw()) > 1.0 - EPS_AXIS_ALIGN
                        && existing_u.as_raw().dot(*u_axis.as_raw()) > 1.0 - EPS_AXIS_ALIGN,
                )
            },
            "catia_e5_plane_candidate_duplicate_scan",
        )? {
            ctx.push_vec(
                &mut candidates,
                (normal, u_axis),
                "catia_e5_plane_candidates",
            )?;
        }
    }
    let mut canonical: Vec<(UnitVector3, UnitVector3, [FiniteReal; 2])> = Vec::new();
    for (normal, mut u_axis) in ctx
        .admit_iter(&candidates, "catia_e5_plane_candidate_canonical_scan")?
        .copied()
    {
        let Some(first) = [u_axis.as_raw().x, u_axis.as_raw().y, u_axis.as_raw().z]
            .into_iter()
            .find(|value| value.abs() > EPS_E5_DECODE_EXACT_GEOMETRY)
        else {
            return Ok(None);
        };
        let uv_scale = if first < 0.0 {
            u_axis = u_axis.reversed();
            [FiniteReal::ONE.negated(); 2]
        } else {
            [FiniteReal::ONE; 2]
        };
        if !ctx.any_by(
            &canonical,
            |(existing_normal, existing_u, _)| {
                Ok(
                    existing_normal.as_raw().dot(*normal.as_raw()) > 1.0 - EPS_AXIS_ALIGN
                        && existing_u.as_raw().dot(*u_axis.as_raw()) > 1.0 - EPS_AXIS_ALIGN,
                )
            },
            "catia_e5_plane_canonical_duplicate_scan",
        )? {
            ctx.push_vec(
                &mut canonical,
                (normal, u_axis, uv_scale),
                "catia_e5_plane_canonical",
            )?;
        }
    }
    Ok((canonical.len() == 1).then(|| canonical[0]))
}

fn e5_native_uv_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &crate::families::e5::graph::E5Pcurve,
) -> Result<Option<[FiniteVector<2>; 2]>, cadmpeg_core::CodecError> {
    let finite = |[start, end]: [[f64; 2]; 2]| {
        let (Some(start), Some(end)) = (FiniteVector::new(start), FiniteVector::new(end)) else {
            return None;
        };
        Some([start, end])
    };
    match pcurve {
        crate::families::e5::graph::E5Pcurve::Line {
            origin,
            direction,
            range,
            ..
        } => Ok(finite(range.map(|parameter| {
            [
                origin[0].get() + parameter.get() * direction[0].get(),
                origin[1].get() + parameter.get() * direction[1].get(),
            ]
        }))),
        crate::families::e5::graph::E5Pcurve::Circle {
            center,
            radius,
            range,
            ..
        } => Ok(finite(range.map(|parameter| {
            let radius = radius.get();
            let angle = parameter.get() / radius;
            [
                center[0].get() + radius * angle.cos(),
                center[1].get() + radius * angle.sin(),
            ]
        }))),
        crate::families::e5::graph::E5Pcurve::Jet { sites, .. } => {
            Ok(sites.first().zip(sites.last()).map(|(first, last)| {
                [
                    FiniteVector::from(first.point),
                    FiniteVector::from(last.point),
                ]
            }))
        }
        crate::families::e5::graph::E5Pcurve::Nurbs {
            degree,
            knots,
            control_points,
            range,
            ..
        } => {
            // The evaluator reads raw lanes, so the copies live only for
            // the two endpoint evaluations.
            let (scalar_knots, _knot_storage) = ctx.try_collect_scoped_vec(
                knots
                    .iter()
                    .map(|knot| Ok::<_, cadmpeg_core::CodecError>(knot.get())),
                "catia_e5_uv_knots",
            )?;
            let (scalar_controls, _control_storage) = ctx.try_collect_scoped_vec(
                control_points
                    .iter()
                    .map(|[u, v]| Ok::<_, cadmpeg_core::CodecError>(Point2::new(u.get(), v.get()))),
                "catia_e5_uv_controls",
            )?;
            let start = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                *degree,
                &scalar_knots,
                &scalar_controls,
                None,
                range[0].get(),
            ))?;
            let end = cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::nurbs_pcurve_uv(
                ctx,
                *degree,
                &scalar_knots,
                &scalar_controls,
                None,
                range[1].get(),
            ))?;
            Ok(start
                .zip(end)
                .map(|(start, end)| [start.into(), end.into()]))
        }
    }
}

fn fit_e5_plane_axes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    origin: FinitePoint3,
    pairs: &[(FiniteVector<2>, FinitePoint3)],
) -> Result<Option<(Vector3, Vector3, f64)>, cadmpeg_core::CodecError> {
    let origin: [f64; 3] = origin.get().into();
    let uv_scale = ctx
        .admit_iter(pairs, "catia_e5_plane_uv_scale_scan")?
        .map(|(uv, _)| uv[0].abs().max(uv[1].abs()))
        .fold(0.0_f64, f64::max);
    if !uv_scale.is_finite() || uv_scale == 0.0 {
        return Ok(None);
    }
    let normalized_uv = |uv: FiniteVector<2>| [uv[0] / uv_scale, uv[1] / uv_scale];
    let suu = ctx
        .admit_iter(pairs, "catia_e5_plane_uu_covariance_scan")?
        .map(|(uv, _)| normalized_uv(*uv)[0].powi(2))
        .sum::<f64>();
    let suv = ctx
        .admit_iter(pairs, "catia_e5_plane_uv_covariance_scan")?
        .map(|(uv, _)| {
            let uv = normalized_uv(*uv);
            uv[0] * uv[1]
        })
        .sum::<f64>();
    let svv = ctx
        .admit_iter(pairs, "catia_e5_plane_vv_covariance_scan")?
        .map(|(uv, _)| normalized_uv(*uv)[1].powi(2))
        .sum::<f64>();
    let determinant = suu * svv - suv * suv;
    let covariance_scale = suu.max(svv);
    let rank_tolerance = f64::EPSILON * covariance_scale * covariance_scale;
    if !determinant.is_finite()
        || !covariance_scale.is_finite()
        || covariance_scale == 0.0
        || determinant <= rank_tolerance
    {
        return Ok(None);
    }
    let mut u = [0.0; 3];
    let mut v = [0.0; 3];
    for axis in 0..3 {
        let bu = ctx
            .admit_iter(pairs, "catia_e5_plane_u_regression_scan")?
            .map(|(uv, point)| {
                normalized_uv(*uv)[0] * ([point.x, point.y, point.z][axis] - origin[axis])
            })
            .sum::<f64>();
        let bv = ctx
            .admit_iter(pairs, "catia_e5_plane_v_regression_scan")?
            .map(|(uv, point)| {
                normalized_uv(*uv)[1] * ([point.x, point.y, point.z][axis] - origin[axis])
            })
            .sum::<f64>();
        u[axis] = (bu * svv - bv * suv) / determinant / uv_scale;
        v[axis] = (suu * bv - suv * bu) / determinant / uv_scale;
    }
    if u.into_iter().chain(v).any(|value| !value.is_finite()) {
        return Ok(None);
    }
    let u_axis = Vector3::from(u);
    let v_axis = Vector3::from(v);
    let residual = plane_frame_residual(ctx, origin, pairs, u_axis, v_axis)?;
    Ok(residual.is_finite().then_some((u_axis, v_axis, residual)))
}

fn fit_rank_one_e5_plane_axes(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    origin: FinitePoint3,
    pairs: &[(FiniteVector<2>, FinitePoint3)],
    normal: Vector3,
) -> Result<Option<(Vector3, Vector3, f64)>, cadmpeg_core::CodecError> {
    let origin: [f64; 3] = origin.get().into();
    let Some((uv, point)) = ctx.find_by(
        pairs,
        |(uv, _)| {
            let norm = uv[0].hypot(uv[1]);
            Ok(norm.is_finite() && norm != 0.0)
        },
        "catia_e5_rank_one_seed_scan",
    )?
    else {
        return Ok(None);
    };
    let uv_norm = uv[0].hypot(uv[1]);
    let q = [uv[0] / uv_norm, uv[1] / uv_norm];
    let displacement = Vector3::new(
        point.x - origin[0],
        point.y - origin[1],
        point.z - origin[2],
    );
    let displacement_norm = displacement.x.hypot(displacement.y).hypot(displacement.z);
    if (displacement_norm - uv_norm).abs() > 2e-3 {
        return Ok(None);
    }
    let Some(mapped_q) = unit_vector(displacement) else {
        return Ok(None);
    };
    let q_direction = mapped_q.as_raw();
    let Some(mapped_r) = unit_vector(Vector3::new(
        normal.y * q_direction.z - normal.z * q_direction.y,
        normal.z * q_direction.x - normal.x * q_direction.z,
        normal.x * q_direction.y - normal.y * q_direction.x,
    )) else {
        return Ok(None);
    };
    let r_direction = mapped_r.as_raw();
    let u_axis = Vector3::new(
        q[0] * q_direction.x - q[1] * r_direction.x,
        q[0] * q_direction.y - q[1] * r_direction.y,
        q[0] * q_direction.z - q[1] * r_direction.z,
    );
    let v_axis = Vector3::new(
        q[1] * q_direction.x + q[0] * r_direction.x,
        q[1] * q_direction.y + q[0] * r_direction.y,
        q[1] * q_direction.z + q[0] * r_direction.z,
    );
    let residual = plane_frame_residual(ctx, origin, pairs, u_axis, v_axis)?;
    Ok(residual.is_finite().then_some((u_axis, v_axis, residual)))
}

fn plane_frame_residual(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    origin: [f64; 3],
    pairs: &[(FiniteVector<2>, FinitePoint3)],
    u_axis: Vector3,
    v_axis: Vector3,
) -> Result<f64, cadmpeg_core::CodecError> {
    Ok(ctx
        .admit_iter(pairs, "catia_e5_plane_frame_residual_scan")?
        .fold(0.0f64, |residual, (uv, point)| {
            let predicted = [
                origin[0] + uv[0] * u_axis.x + uv[1] * v_axis.x,
                origin[1] + uv[0] * u_axis.y + uv[1] * v_axis.y,
                origin[2] + uv[0] * u_axis.z + uv[1] * v_axis.z,
            ];
            let error = distance(predicted, point.get().into());
            if error.is_finite() {
                residual.max(error)
            } else {
                f64::INFINITY
            }
        }))
}

fn canonical_direction(mut direction: Vector3) -> Vector3 {
    let first = [direction.x, direction.y, direction.z]
        .into_iter()
        .find(|value| value.abs() > EPS_E5_DECODE_EXACT_GEOMETRY)
        .unwrap_or(1.0);
    if first < 0.0 {
        direction = Vector3::new(-direction.x, -direction.y, -direction.z);
    }
    direction
}

fn attach_e5_free_vertices(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    let body_id = ctx
        .copy_retained_text("catia:e5:body#unbound-points", "catia_e5_free_body_id")
        .and_then(|text| BodyId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    let region_id = ctx
        .copy_retained_text("catia:e5:region#unbound-points", "catia_e5_free_region_id")
        .and_then(|text| RegionId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    let shell_id = ctx
        .copy_retained_text("catia:e5:shell#unbound-points", "catia_e5_free_shell_id")
        .and_then(|text| ShellId::mint(text).map_err(cadmpeg_core::CodecError::malformed))?;
    for id in [body_id.as_str(), region_id.as_str(), shell_id.as_str()] {
        annotate(
            ctx,
            annotations,
            id,
            "e5_0d_03",
            0,
            "unbound_point_owner",
            Exactness::Inferred,
        )?;
    }
    let mut regions = Vec::new();
    ctx.push_vec(
        &mut regions,
        region_id.try_clone_for_decode(ctx, "catia_e5_free_body_region_id")?,
        "catia_e5_free_body_regions",
    )?;
    let mut shells = Vec::new();
    ctx.push_vec(
        &mut shells,
        shell_id.try_clone_for_decode(ctx, "catia_e5_free_region_shell_id")?,
        "catia_e5_free_region_shells",
    )?;
    let free_vertices = ctx.try_collect_vec(
        ir.model.vertices.iter().map(|vertex| {
            vertex
                .id
                .try_clone_for_decode(ctx, "catia_e5_free_vertex_id")
        }),
        "catia_e5_free_vertices",
    )?;
    admission.reserve_entity(&mut ir.model.bodies, "catia_e5_model_bodies")?;
    ir.model.bodies.push(Body {
        id: body_id.try_clone_for_decode(ctx, "catia_e5_free_body_record_id")?,
        kind: BodyKind::Wire,
        regions,
        transform: None,
        name: None,
        color: None,
        visible: None,
    });
    admission.reserve_entity(&mut ir.model.regions, "catia_e5_model_regions")?;
    ir.model.regions.push(Region {
        id: region_id.try_clone_for_decode(ctx, "catia_e5_free_region_record_id")?,
        body: body_id,
        shells,
    });
    admission.reserve_entity(&mut ir.model.shells, "catia_e5_model_shells")?;
    ir.model.shells.push(
        match Shell::new(shell_id, region_id, Vec::new(), Vec::new(), free_vertices) {
            Ok(shell) => shell,
            Err(_) => {
                return Ok(());
            }
        },
    );
    Ok(())
}

struct E5IntersectionSidePlan {
    surface: SurfaceId,
    pcurve: PcurveGeometry,
    pcurve_range: [f64; 2],
    curve: CurveGeometry,
    curve_range: [f64; 2],
}

#[derive(Clone)]
struct E5OccurrenceIntersectionSide {
    surface: SurfaceId,
    pcurve: PcurveGeometry,
    pcurve_range: [f64; 2],
    curve: Option<(CurveGeometry, [f64; 2])>,
}

/// Boundary lowering plan built by [`plan_e5_boundary`].
struct E5BoundaryPlan<'a> {
    faces: Vec<E5FacePlan<'a>>,
    pcurves: BTreeMap<u32, (PcurveGeometry, [f64; 2])>,
    /// Whether each native pcurve occurrence runs opposite to its edge's
    /// stored endpoint order.
    pcurve_use_reversed: BTreeMap<(u32, usize), bool>,
    edge_curves: BTreeMap<u32, (CurveGeometry, [f64; 2])>,
    surface_curves: BTreeMap<u32, (SurfaceId, PcurveGeometry, [f64; 2])>,
    intersections: BTreeMap<u32, IntcurveSupportContext>,
}

struct E5FacePlan<'a> {
    source: &'a crate::families::e5::graph::E5Face,
    loops: Vec<E5LoopPlan<'a>>,
}

struct E5LoopPlan<'a> {
    source: &'a crate::families::e5::graph::E5Loop,
    members: Vec<E5MemberPlan<'a>>,
}

struct E5MemberPlan<'a> {
    source: &'a crate::families::e5::graph::E5LoopMember,
    orientation: &'a crate::families::e5::graph::E5OrientedMember,
    id: CoedgeId,
}

impl<'a> E5LoopPlan<'a> {
    fn admit(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        source: &'a crate::families::e5::graph::E5Loop,
    ) -> Result<Option<Self>, cadmpeg_core::CodecError> {
        let Some(oriented) = source.resolved_members() else {
            return Ok(None);
        };
        if source.members.is_empty() || oriented.len() != source.members.len() {
            return Ok(None);
        }
        let mut seen = ctx.alloc_filled(source.members.len(), false, "catia_e5_loop_plan_seen")?;
        let mut members = ctx.vector_storage(oriented.len(), "catia_e5_loop_plan_members")?;
        for orientation in ctx.admit_iter(oriented, "catia_e5_loop_plan_orientation_scan")? {
            let index = orientation.serialized_index;
            let (Some(member), Some(seen)) = (source.members.get(index), seen.get_mut(index))
            else {
                return Ok(None);
            };
            if std::mem::replace(seen, true) {
                return Ok(None);
            }
            ctx.push_vec(
                &mut members,
                E5MemberPlan {
                    source: member,
                    orientation,
                    id: CoedgeId::compose(
                        &cadmpeg_ir::identity_namespace!("catia", "e5", "coedge"),
                        cadmpeg_ir::ids::IdentityKey::from(source.record_id).dash(index),
                    ),
                },
                "catia_e5_loop_plan_members",
            )?;
        }
        Ok(Some(Self { source, members }))
    }
}

/// Body/region/shell ownership resolved by [`resolve_e5_ownership`].
struct E5Ownership {
    bodies: Vec<E5BodyPlan>,
    face_shell: HashMap<u32, ShellId>,
}

/// The sorted, distinct vertex references and the model points in the same
/// order. A reference's position is its vertex ordinal.
#[derive(Clone, Copy)]
struct E5VertexPoints<'a> {
    refs: &'a [u32],
    points: &'a [Point],
}

impl E5VertexPoints<'_> {
    fn point(
        self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        vertex: u32,
    ) -> Result<Option<Point3>, cadmpeg_core::CodecError> {
        Ok(e5_vertex_ordinal(ctx, self.refs, vertex)?
            .and_then(|index| self.points.get(index))
            .map(|point| point.position().get()))
    }
}

/// The ordinal of a referenced vertex in the sorted, distinct reference list.
fn e5_vertex_ordinal(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    vertex_refs: &[u32],
    vertex: u32,
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    Ok(ctx
        .binary_search(vertex_refs, &vertex, "catia_e5_vertex_ref_search")?
        .ok())
}

/// The model vertex id of a referenced vertex, minted from its ordinal as
/// the vertex layer mints it.
fn e5_vertex_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    vertex_refs: &[u32],
    vertex: u32,
    operation: &'static str,
) -> Result<VertexId, cadmpeg_core::CodecError> {
    let Some(index) = e5_vertex_ordinal(ctx, vertex_refs, vertex)? else {
        return Err(cadmpeg_core::CodecError::malformed(
            "E5 edge names an unreferenced vertex",
        ));
    };
    crate::resource::compose_index_id(
        ctx,
        &cadmpeg_ir::identity_namespace!("catia", "e5", "v"),
        index,
        VertexId::mint,
        operation,
    )
}

/// The model edge id of an edge record.
fn e5_edge_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record_id: u32,
    operation: &'static str,
) -> Result<EdgeId, cadmpeg_core::CodecError> {
    crate::resource::compose_u32_id(
        ctx,
        &cadmpeg_ir::identity_namespace!("catia", "e5", "edge"),
        record_id,
        EdgeId::mint,
        operation,
    )
}

fn transfer_e5_topology(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    (ir, annotations): (
        &mut CadIr,
        &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    ),
    topology: &crate::families::e5::graph::E5Topology,
    decoded_surfaces: &[crate::families::e5::records::E5Surface],
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
    unused_surfaces: &mut Vec<Surface>,
) -> Result<bool, cadmpeg_core::CodecError> {
    if topology.vertex_refs.len() != ir.model.vertices.len()
        || topology.vertex_refs.len() != ir.model.points.len()
        || topology.vertex_refs.is_empty()
    {
        return Ok(false);
    }

    let curves = std::mem::take(&mut ir.model.curves);
    for curve in ctx.admit_iter(&curves, "catia_e5_transfer_curve_scan")? {
        annotations.remove_entity(ctx, curve.id.as_str())?;
    }

    // The surface index, the boundary plan's own collections and the
    // ownership plan are scratch; the emitters copy what the model keeps.
    let mut scratch = ctx.reserve_scoped(0, "catia_e5_transfer_scratch")?;
    let mut surface_for_ref = HashMap::new();
    for (index, surface) in ctx
        .admit_iter(decoded_surfaces, "catia_e5_transfer_surface_scan")?
        .enumerate()
    {
        scratch.with_storage(|| {
            ctx.insert_hash_map(
                &mut surface_for_ref,
                surface.record_id,
                (
                    crate::resource::compose_index_id(
                        ctx,
                        &cadmpeg_ir::identity_namespace!("catia", "e5", "surf"),
                        index,
                        SurfaceId::mint,
                        "catia_e5_transfer_surface_id",
                    )?,
                    surface,
                ),
                "catia_e5_transfer_surface_refs",
            )
        })?;
    }

    let vertices = E5VertexPoints {
        refs: &topology.vertex_refs,
        points: &ir.model.points,
    };
    let Some(boundary) = plan_e5_boundary(
        ctx,
        &mut scratch,
        topology,
        &surface_for_ref,
        vertices,
        refusal,
    )?
    else {
        return Ok(false);
    };
    prune_e5_unused_surfaces(
        ctx,
        ir,
        annotations,
        topology,
        &surface_for_ref,
        (&boundary.intersections, &boundary.surface_curves),
        unused_surfaces,
    )?;

    let Some(e5_ownership) = scratch.with_storage(|| resolve_e5_ownership(ctx, topology))? else {
        return Ok(false);
    };
    let E5Ownership { bodies, face_shell } = e5_ownership;

    if let Err(error) = emit_e5_curves_and_edges(
        ctx,
        crate::families::e5::decode::EmitE5CurvesAndEdgesInputs {
            ir,
            annotations,
            topology,
            edge_curves: &boundary.edge_curves,
            intersections: &boundary.intersections,
            surface_curves: &boundary.surface_curves,
            admission,
        },
    ) {
        return match error {
            cadmpeg_core::CodecError::ResourceLimit(_) => Err(error),
            _ => Ok(false),
        };
    }
    if let Err(error) = emit_e5_pcurves(ctx, ir, annotations, &boundary.pcurves, admission) {
        return match error {
            cadmpeg_core::CodecError::ResourceLimit(_) => Err(error),
            _ => Ok(false),
        };
    }
    if let Err(error) = emit_e5_bodies(ctx, ir, annotations, &bodies, admission) {
        return match error {
            cadmpeg_core::CodecError::ResourceLimit(_) => Err(error),
            _ => Ok(false),
        };
    }
    if !emit_e5_faces_loops_coedges(
        ctx,
        crate::families::e5::decode::EmitE5FacesLoopsCoedgesInputs {
            ir,
            annotations,
            topology,
            surface_for_ref: &surface_for_ref,
            face_shell: &face_shell,
            boundary: &boundary,
            admission,
        },
    )? {
        return Ok(false);
    }
    Ok(true)
}

/// Lowers every face loop to boundary curves, pcurves, and intersection contexts,
/// or returns `None` when any binding fails admission.
///
/// The plan's collections and the copies made for it live in `scratch`. The
/// evaluated geometry is built by readers that also record retained refusal
/// notes, so it is charged where it is built.
fn plan_e5_boundary<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    scratch: &mut ScopedReservation<'_>,
    topology: &'a crate::families::e5::graph::E5Topology,
    surface_for_ref: &HashMap<u32, (SurfaceId, &crate::families::e5::records::E5Surface)>,
    vertices: E5VertexPoints<'_>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<E5BoundaryPlan<'a>>, cadmpeg_core::CodecError> {
    let mut faces = Vec::new();
    for face in ctx.admit_iter(&topology.faces, "catia_e5_boundary_face_plan_scan")? {
        let mut loops = Vec::new();
        for source in ctx.admit_iter(&face.loops, "catia_e5_boundary_loop_plan_scan")? {
            let Some(loop_plan) = scratch.with_storage(|| E5LoopPlan::admit(ctx, source))? else {
                return Ok(None);
            };
            ctx.push_scoped_vec(scratch, &mut loops, loop_plan, "catia_e5_face_plan_loops")?;
        }
        ctx.push_scoped_vec(
            scratch,
            &mut faces,
            E5FacePlan {
                source: face,
                loops,
            },
            "catia_e5_boundary_face_plans",
        )?;
    }
    let mut pcurves = BTreeMap::<u32, (PcurveGeometry, [f64; 2])>::new();
    let mut pcurve_use_reversed = BTreeMap::<(u32, usize), bool>::new();
    let mut edge_curves = BTreeMap::<u32, (CurveGeometry, [f64; 2])>::new();
    let mut surface_curves = BTreeMap::<u32, (SurfaceId, PcurveGeometry, [f64; 2])>::new();
    let mut occurrence_intersection_sides =
        BTreeMap::<u32, Vec<E5OccurrenceIntersectionSide>>::new();
    for face in ctx.admit_iter(&topology.faces, "catia_e5_boundary_face_member_scan")? {
        let Some((face_surface_id, decoded_surface)) = ctx.get_hash_map(
            surface_for_ref,
            &face.surface,
            "catia_e5_transfer_surface_refs",
        )?
        else {
            return Ok(None);
        };
        for loop_ in ctx.admit_iter(&face.loops, "catia_e5_boundary_loop_member_scan")? {
            for (member_index, member) in ctx
                .admit_iter(&loop_.members, "catia_e5_boundary_member_scan")?
                .enumerate()
            {
                let pcurve_ref = member.pcurve;
                let edge_ref = member.edge_use;
                let Some(edge) =
                    ctx.get_btree_map(&topology.edges, &edge_ref, "catia_e5_boundary_edge_lookup")?
                else {
                    return Ok(None);
                };
                let Some(support) = ctx.get_btree_map(
                    &topology.curve_supports,
                    &edge.support,
                    "catia_e5_boundary_support_lookup",
                )?
                else {
                    return Ok(None);
                };
                let Some(pcurve) = ctx.get_btree_map(
                    &topology.pcurves,
                    &pcurve_ref,
                    "catia_e5_boundary_pcurve_lookup",
                )?
                else {
                    return Ok(None);
                };
                let Some((geometry, range, endpoints)) =
                    e5_pcurve_on_surface(ctx, pcurve, decoded_surface, refusal)?
                else {
                    return Ok(None);
                };
                let (Some(start), Some(end)) = (
                    vertices.point(ctx, edge.start_vertex)?,
                    vertices.point(ctx, edge.end_vertex)?,
                ) else {
                    return Ok(None);
                };
                let forward = endpoints[0].distance(start).max(endpoints[1].distance(end));
                let reverse_error = endpoints[0].distance(end).max(endpoints[1].distance(start));
                // A degenerate native bound has no parameter sign. Endpoint
                // positions are an independent constraint, but they only
                // select a direction when exactly one order meets the model
                // tolerance. Never choose the smaller of two failing errors.
                let reversed =
                    e5_stored_pcurve_reversed(ctx, topology, edge_ref, pcurve_ref, range)?
                        .or_else(|| unique_endpoint_direction(forward, reverse_error));
                let Some(reversed) = reversed else {
                    return Ok(None);
                };
                let repeated = scratch.with_storage(|| {
                    ctx.insert_btree_map(
                        &mut pcurve_use_reversed,
                        (loop_.record_id, member_index),
                        reversed,
                        "catia_e5_boundary_occurrence_senses",
                    )
                })?;
                if repeated.is_some()
                    || if reversed { reverse_error } else { forward } > E5_ENDPOINT_MATCH_TOLERANCE
                {
                    return Ok(None);
                }
                let oriented_pcurve = if reversed {
                    let (record, _reservation) = ctx.format_scoped(
                        format_args!(
                            "e5 boundary pcurve of loop record {} member {member_index}",
                            loop_.record_id
                        ),
                        "catia_e5_reverse_pcurve_label",
                    )?;
                    let Some(reversed) = crate::nurbs::reverse_pcurve_geometry(
                        ctx, &geometry, range, refusal, &record,
                    )?
                    else {
                        return Ok(None);
                    };
                    reversed
                } else {
                    scratch.with_storage(|| {
                        geometry.try_clone_for_decode(ctx, "catia_e5_oriented_pcurve_copy")
                    })?
                };
                let lifted_curve = if let Some((mut curve, mut curve_range)) = e5_boundary_curve(
                    ctx,
                    &decoded_surface.geometry,
                    pcurve,
                    &geometry,
                    (range, endpoints),
                    decoded_surface.uv_scale,
                    refusal,
                )? {
                    if reversed {
                        let (record, _reservation) = ctx.format_scoped(
                            format_args!(
                                "e5 boundary curve of loop record {} member {member_index}",
                                loop_.record_id
                            ),
                            "catia_e5_reverse_curve_label",
                        )?;
                        let Some(reversed_curve) = crate::nurbs::reverse_curve_geometry(
                            ctx,
                            &curve,
                            curve_range,
                            refusal,
                            &record,
                        )?
                        else {
                            return Ok(None);
                        };
                        (curve, curve_range) = reversed_curve;
                    }
                    Some((curve, curve_range))
                } else {
                    None
                };
                if support.is_intersection() {
                    scratch.with_storage(|| {
                        push_occurrence_intersection_side(
                            ctx,
                            &mut occurrence_intersection_sides,
                            edge_ref,
                            (face_surface_id, &oriented_pcurve, range),
                            lifted_curve.as_ref(),
                        )
                    })?;
                }
                if let Some((curve, curve_range)) = lifted_curve {
                    if !support.is_intersection() {
                        if let Some(existing) =
                            ctx.get_btree_map(&edge_curves, &edge_ref, "catia_e5_edge_curve_plan")?
                        {
                            // Both carriers were built and charged by the
                            // lift; core has no charged geometry equality.
                            if existing != &(curve, curve_range) {
                                return Ok(None);
                            }
                        } else {
                            scratch.with_storage(|| {
                                ctx.insert_btree_map(
                                    &mut edge_curves,
                                    edge_ref,
                                    (curve, curve_range),
                                    "catia_e5_edge_curve_plan",
                                )
                            })?;
                        }
                    }
                } else if !support.is_intersection()
                    && !ctx.contains_key_btree_map(
                        &surface_curves,
                        &edge_ref,
                        "catia_e5_surface_curve_plan",
                    )?
                {
                    scratch.with_storage(|| {
                        let surface_id = face_surface_id
                            .try_clone_for_decode(ctx, "catia_e5_surface_curve_surface_id")?;
                        ctx.insert_btree_map(
                            &mut surface_curves,
                            edge_ref,
                            (surface_id, oriented_pcurve, range),
                            "catia_e5_surface_curve_plan",
                        )
                    })?;
                }
                if let Some((existing, existing_range)) =
                    ctx.get_btree_map(&pcurves, &pcurve_ref, "catia_e5_pcurve_plan")?
                {
                    // Both pcurves were built and charged by the lift; core
                    // has no charged geometry equality.
                    if existing != &geometry || existing_range != &range {
                        return Ok(None);
                    }
                } else {
                    scratch.with_storage(|| {
                        ctx.insert_btree_map(
                            &mut pcurves,
                            pcurve_ref,
                            (geometry, range),
                            "catia_e5_pcurve_plan",
                        )
                    })?;
                }
            }
        }
    }
    let mut intersection_sides = BTreeMap::<u32, BTreeMap<u32, E5IntersectionSidePlan>>::new();
    for (&edge_ref, edge) in ctx.admit_iter(&topology.edges, "catia_e5_intersection_edge_scan")? {
        let Some(support) = ctx.get_btree_map(
            &topology.curve_supports,
            &edge.support,
            "catia_e5_intersection_support_lookup",
        )?
        else {
            return Ok(None);
        };
        let crate::families::e5::graph::E5CurveSupportKind::Intersection(sides) = support.kind
        else {
            continue;
        };
        let (Some(start), Some(end)) = (
            vertices.point(ctx, edge.start_vertex)?,
            vertices.point(ctx, edge.end_vertex)?,
        ) else {
            return Ok(None);
        };
        for pcurve_ref in sides {
            let Some(pcurve) = ctx.get_btree_map(
                &topology.pcurves,
                &pcurve_ref,
                "catia_e5_intersection_pcurve_lookup",
            )?
            else {
                continue;
            };
            let Some((surface_id, decoded_surface)) = ctx.get_hash_map(
                surface_for_ref,
                &pcurve.surface_record_id(),
                "catia_e5_transfer_surface_refs",
            )?
            else {
                continue;
            };
            let Some((geometry, range, endpoints)) =
                e5_pcurve_on_surface(ctx, pcurve, decoded_surface, refusal)?
            else {
                // A known intersection pcurve that cannot be normalized must
                // reject the topology route; omitting one side would claim a
                // closed graph with incomplete carrier geometry.
                return Ok(None);
            };
            let forward = endpoints[0].distance(start).max(endpoints[1].distance(end));
            let reverse_error = endpoints[0].distance(end).max(endpoints[1].distance(start));
            let reversed = e5_stored_pcurve_reversed(ctx, topology, edge_ref, pcurve_ref, range)?
                .or_else(|| unique_endpoint_direction(forward, reverse_error));
            let Some(reversed) = reversed else {
                continue;
            };
            if if reversed { reverse_error } else { forward } > E5_ENDPOINT_MATCH_TOLERANCE {
                continue;
            }
            let Some((mut curve, mut curve_range)) = e5_boundary_curve(
                ctx,
                &decoded_surface.geometry,
                pcurve,
                &geometry,
                (range, endpoints),
                decoded_surface.uv_scale,
                refusal,
            )?
            else {
                continue;
            };
            if reversed {
                let (record, _reservation) = ctx.format_scoped(
                    format_args!("e5 boundary curve of edge {edge_ref} pcurve {pcurve_ref}"),
                    "catia_e5_intersection_reverse_curve_label",
                )?;
                let Some(reversed_curve) = crate::nurbs::reverse_curve_geometry(
                    ctx,
                    &curve,
                    curve_range,
                    refusal,
                    &record,
                )?
                else {
                    continue;
                };
                (curve, curve_range) = reversed_curve;
            }
            let pcurve = if reversed {
                let (record, _reservation) = ctx.format_scoped(
                    format_args!("e5 boundary pcurve of edge {edge_ref} pcurve {pcurve_ref}"),
                    "catia_e5_intersection_reverse_pcurve_label",
                )?;
                let Some(reversed) =
                    crate::nurbs::reverse_pcurve_geometry(ctx, &geometry, range, refusal, &record)?
                else {
                    continue;
                };
                reversed
            } else {
                geometry
            };
            scratch.with_storage(|| {
                let side = E5IntersectionSidePlan {
                    surface: surface_id
                        .try_clone_for_decode(ctx, "catia_e5_intersection_surface_id")?,
                    pcurve,
                    pcurve_range: range,
                    curve,
                    curve_range,
                };
                if let Some(sides) = ctx.get_mut_btree_map(
                    &mut intersection_sides,
                    &edge_ref,
                    "catia_e5_intersection_edge_keys",
                )? {
                    ctx.insert_btree_map(
                        sides,
                        pcurve_ref,
                        side,
                        "catia_e5_intersection_side_keys",
                    )?;
                } else {
                    let mut sides = BTreeMap::new();
                    ctx.insert_btree_map(
                        &mut sides,
                        pcurve_ref,
                        side,
                        "catia_e5_intersection_side_keys",
                    )?;
                    ctx.insert_btree_map(
                        &mut intersection_sides,
                        edge_ref,
                        sides,
                        "catia_e5_intersection_edge_keys",
                    )?;
                }
                Ok::<(), cadmpeg_core::CodecError>(())
            })?;
        }
    }

    let mut intersections = BTreeMap::<u32, IntcurveSupportContext>::new();
    for (&edge_ref, sides) in
        ctx.admit_iter(&intersection_sides, "catia_e5_intersection_side_edge_scan")?
    {
        let Some(edge) = ctx.get_btree_map(
            &topology.edges,
            &edge_ref,
            "catia_e5_intersection_edge_lookup",
        )?
        else {
            return Ok(None);
        };
        let Some(support) = ctx.get_btree_map(
            &topology.curve_supports,
            &edge.support,
            "catia_e5_intersection_support_lookup",
        )?
        else {
            return Ok(None);
        };
        let [left_ref, right_ref] = support.pcurves() else {
            continue;
        };
        let (Some(left), Some(right)) = (
            ctx.get_btree_map(sides, left_ref, "catia_e5_intersection_side_keys")?,
            ctx.get_btree_map(sides, right_ref, "catia_e5_intersection_side_keys")?,
        ) else {
            continue;
        };
        let same_parameterization =
            parameter_range_agreement_tolerance(left.curve_range, right.curve_range).is_some();
        let same_carrier = equivalent_e5_curve_carriers(&left.curve, &right.curve);
        let same_ordered_sweep = e5_circle_carriers_have_same_ordered_sweep(
            ctx,
            &left.curve,
            left.curve_range,
            &right.curve,
            right.curve_range,
        )?;
        if !(same_carrier && same_parameterization || same_ordered_sweep) {
            continue;
        }
        let Some(context) = scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut edge_curves,
                edge_ref,
                (
                    left.curve
                        .try_clone_for_decode(ctx, "catia_e5_boundary_curve_copy")?,
                    left.curve_range,
                ),
                "catia_e5_edge_curve_plan",
            )?;
            let [left_side, right_side] = [left, right].map(|side| {
                Ok::<_, cadmpeg_core::CodecError>(IntcurveSupportSide {
                    surface: Some(
                        side.surface.try_clone_for_decode(
                            ctx,
                            "catia_e5_intersection_context_surface_id",
                        )?,
                    ),
                    pcurve: Some(SupportPcurve::new(
                        side.pcurve
                            .try_clone_for_decode(ctx, "catia_e5_intersection_context_pcurve")?,
                        DirectedParameterRange::new(side.pcurve_range).ok(),
                    )),
                })
            });
            Ok::<_, cadmpeg_core::CodecError>(
                IntcurveSupportContext::try_new(
                    [left_side?, right_side?],
                    left.curve_range,
                    std::array::from_fn(|_| Vec::new()),
                )
                .ok(),
            )
        })?
        else {
            return Ok(None);
        };
        scratch.with_storage(|| {
            ctx.insert_btree_map(
                &mut intersections,
                edge_ref,
                context,
                "catia_e5_intersection_plan",
            )
        })?;
    }
    for (&edge_ref, sides) in ctx.admit_iter(
        &occurrence_intersection_sides,
        "catia_e5_occurrence_intersection_scan",
    )? {
        if ctx.contains_key_btree_map(&intersections, &edge_ref, "catia_e5_intersection_plan")? {
            continue;
        }
        let Some(edge) = ctx.get_btree_map(
            &topology.edges,
            &edge_ref,
            "catia_e5_occurrence_edge_lookup",
        )?
        else {
            return Ok(None);
        };
        let Some(support) = ctx.get_btree_map(
            &topology.curve_supports,
            &edge.support,
            "catia_e5_occurrence_support_lookup",
        )?
        else {
            return Ok(None);
        };
        let cache = e5_occurrence_intersection_cache(ctx, sides)?;
        let support_range = support.range.map(FiniteReal::get);
        let solved_range = cache.as_ref().map_or(support_range, |(_, range)| *range);
        let Some(context) = scratch.with_storage(|| {
            e5_support_occurrence_intersection_context(ctx, support_range, solved_range, sides)
        })?
        else {
            if let [side] = sides.as_slice() {
                if !ctx.contains_key_btree_map(
                    &surface_curves,
                    &edge_ref,
                    "catia_e5_surface_curve_plan",
                )? {
                    scratch.with_storage(|| {
                        let surface_id = side
                            .surface
                            .try_clone_for_decode(ctx, "catia_e5_surface_curve_surface_id")?;
                        let pcurve = side
                            .pcurve
                            .try_clone_for_decode(ctx, "catia_e5_surface_curve_pcurve")?;
                        ctx.insert_btree_map(
                            &mut surface_curves,
                            edge_ref,
                            (surface_id, pcurve, side.pcurve_range),
                            "catia_e5_surface_curve_plan",
                        )
                    })?;
                }
            }
            continue;
        };
        scratch.with_storage(|| {
            let curve = match cache {
                Some((curve, range)) => (
                    curve.try_clone_for_decode(ctx, "catia_e5_boundary_curve_copy")?,
                    range,
                ),
                None => (
                    CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                    solved_range,
                ),
            };
            ctx.insert_btree_map(
                &mut edge_curves,
                edge_ref,
                curve,
                "catia_e5_edge_curve_plan",
            )?;
            ctx.insert_btree_map(
                &mut intersections,
                edge_ref,
                context,
                "catia_e5_intersection_plan",
            )
        })?;
    }

    for (&edge_ref, (_, _, range)) in
        ctx.admit_iter(&surface_curves, "catia_e5_missing_edge_curve_scan")?
    {
        scratch.with_storage(|| {
            ctx.entry_btree_map(&mut edge_curves, edge_ref, "catia_e5_edge_curve_plan")
                .map(|entry| {
                    entry.or_insert((
                        CurveGeometry::Solved(SolvedCurveGeometry::Unknown { record: None }),
                        *range,
                    ));
                })
        })?;
    }
    Ok(Some(E5BoundaryPlan {
        faces,
        pcurves,
        pcurve_use_reversed,
        edge_curves,
        surface_curves,
        intersections,
    }))
}

/// Records one occurrence's side of an intersection edge. An edge with more
/// than two distinct sides has no intersection context and no surface curve,
/// so a third distinct side ends the comparisons for that edge.
fn push_occurrence_intersection_side(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sides_by_edge: &mut BTreeMap<u32, Vec<E5OccurrenceIntersectionSide>>,
    edge_ref: u32,
    (surface, pcurve, pcurve_range): (&SurfaceId, &PcurveGeometry, [f64; 2]),
    curve: Option<&(CurveGeometry, [f64; 2])>,
) -> Result<(), cadmpeg_core::CodecError> {
    const MAX_DISTINCT_SIDES: usize = 3;
    if let Some(sides) =
        ctx.get_btree_map(sides_by_edge, &edge_ref, "catia_e5_occurrence_side_keys")?
    {
        if sides.len() == MAX_DISTINCT_SIDES {
            return Ok(());
        }
        // At most two stored sides are compared; each pcurve was built and
        // charged by the lift, and core has no charged geometry equality.
        let repeated = ctx.any_by(
            sides,
            |existing| {
                Ok(ctx.equal(
                    &existing.surface,
                    surface,
                    "catia_e5_occurrence_side_surface",
                )? && existing.pcurve == *pcurve
                    && existing.pcurve_range == pcurve_range)
            },
            "catia_e5_occurrence_side_duplicate_scan",
        )?;
        if repeated {
            return Ok(());
        }
    }
    let side = E5OccurrenceIntersectionSide {
        surface: surface.try_clone_for_decode(ctx, "catia_e5_occurrence_surface_id")?,
        pcurve: pcurve.try_clone_for_decode(ctx, "catia_e5_occurrence_pcurve")?,
        pcurve_range,
        curve: curve
            .map(|(curve, range)| {
                Ok::<_, cadmpeg_core::CodecError>((
                    curve.try_clone_for_decode(ctx, "catia_e5_boundary_curve_copy")?,
                    *range,
                ))
            })
            .transpose()?,
    };
    ctx.push_btree_group(
        sides_by_edge,
        edge_ref,
        side,
        "catia_e5_occurrence_side_keys",
        "catia_e5_occurrence_sides",
    )
}

/// Drops surfaces no face, intersection side, or surface curve references.
fn prune_e5_unused_surfaces(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    topology: &crate::families::e5::graph::E5Topology,
    surface_for_ref: &HashMap<u32, (SurfaceId, &crate::families::e5::records::E5Surface)>,
    (intersections, surface_curves): E5CurvePlans<'_>,
    unused_surfaces: &mut Vec<Surface>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut storage = ctx.reserve_scoped(0, "catia_e5_used_surfaces")?;
    let mut used_surfaces = HashSet::new();
    for face in ctx.admit_iter(&topology.faces, "catia_e5_used_face_surface_scan")? {
        if let Some((id, _)) = ctx.get_hash_map(
            surface_for_ref,
            &face.surface,
            "catia_e5_transfer_surface_refs",
        )? {
            storage.with_storage(|| {
                ctx.insert_hash_set(&mut used_surfaces, id.as_str(), "catia_e5_used_surfaces")
            })?;
        }
    }
    for (_, context) in ctx.admit_iter(intersections, "catia_e5_used_intersection_scan")? {
        for side in ctx.admit_iter(context.sides(), "catia_e5_used_intersection_side_scan")? {
            if let Some(surface) = side.surface.as_ref() {
                storage.with_storage(|| {
                    ctx.insert_hash_set(
                        &mut used_surfaces,
                        surface.as_str(),
                        "catia_e5_used_surfaces",
                    )
                })?;
            }
        }
    }
    for (_, (surface, _, _)) in
        ctx.admit_iter(surface_curves, "catia_e5_used_surface_curve_scan")?
    {
        storage.with_storage(|| {
            ctx.insert_hash_set(
                &mut used_surfaces,
                surface.as_str(),
                "catia_e5_used_surfaces",
            )
        })?;
    }
    // Used surfaces move to the front in their order; the unused tail moves
    // out. A rollback re-sorts the surfaces by source ordinal.
    let surfaces = &mut ir.model.surfaces;
    let mut kept = 0;
    for read in ctx.admit_iter(0..surfaces.len(), "catia_e5_used_surface_partition")? {
        if ctx.contains_hash_set(
            &used_surfaces,
            surfaces[read].id.as_str(),
            "catia_e5_used_surfaces",
        )? {
            surfaces.swap(kept, read);
            kept += 1;
        }
    }
    let mut unused = ctx.split_off_vec(surfaces, kept, "catia_e5_unused_surfaces")?;
    for surface in ctx.admit_iter(&unused, "catia_e5_unused_surface_scan")? {
        annotations.remove_entity(ctx, surface.id.as_str())?;
    }
    ctx.append_vec(unused_surfaces, &mut unused, "catia_e5_unused_surfaces")
}

/// Resolves body face groupings into region/shell components, or `None` on
/// failure. The caller holds the plan as scratch.
fn resolve_e5_ownership(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::families::e5::graph::E5Topology,
) -> Result<Option<E5Ownership>, cadmpeg_core::CodecError> {
    let mut body_faces = Vec::new();
    if topology.bodies.is_empty() {
        let mut faces = ctx.vector_storage(topology.faces.len(), "catia_e5_ownership_faces")?;
        for face in ctx.admit_iter(&topology.faces, "catia_e5_ownership_face_record_scan")? {
            ctx.push_vec(&mut faces, face.record_id, "catia_e5_ownership_faces")?;
        }
        ctx.push_vec(&mut body_faces, (None, faces), "catia_e5_ownership_bodies")?;
    } else {
        for body in ctx.admit_iter(&topology.bodies, "catia_e5_ownership_body_record_scan")? {
            let faces = ctx.copy_slice(&body.faces, "catia_e5_ownership_faces")?;
            ctx.push_vec(
                &mut body_faces,
                (Some(body.record_id), faces),
                "catia_e5_ownership_bodies",
            )?;
        }
    }
    let Some(bodies) = e5_ownership_plan(ctx, topology, &body_faces)? else {
        return Ok(None);
    };
    let mut face_shell = HashMap::new();
    for (body, plan) in ctx
        .admit_iter(&bodies, "catia_e5_ownership_body_plan_scan")?
        .enumerate()
    {
        for (component, faces) in ctx
            .admit_iter(&plan.components, "catia_e5_ownership_component_scan")?
            .enumerate()
        {
            let shell = ShellId::mint(ctx.format_retained(
                format_args!("catia:e5:shell#{body}-{component}"),
                "catia_e5_ownership_shell_id",
            )?)
            .map_err(cadmpeg_core::CodecError::malformed)?;
            for face in ctx.admit_iter(faces, "catia_e5_ownership_face_shell_scan")? {
                ctx.insert_hash_map(
                    &mut face_shell,
                    *face,
                    shell.try_clone_for_decode(ctx, "catia_e5_face_shell_id")?,
                    "catia_e5_face_shells",
                )?;
            }
        }
    }
    Ok(Some(E5Ownership { bodies, face_shell }))
}

/// Emits the boundary curve, intersection/surface-curve procedural, and edge layers.
struct EmitE5CurvesAndEdgesInputs<
    'input0,
    'input1,
    'input2,
    'input5,
    'input6,
    'input7,
    'input8,
    'input9,
    'input10,
    AnnotationAccount,
> {
    ir: &'input0 mut CadIr,
    annotations: &'input1 mut AnnotationBuilder<AnnotationAccount>,
    topology: &'input2 crate::families::e5::graph::E5Topology,
    edge_curves: &'input5 BTreeMap<u32, (CurveGeometry, [f64; 2])>,
    intersections: &'input6 BTreeMap<u32, IntcurveSupportContext>,
    surface_curves: &'input7 BTreeMap<u32, (SurfaceId, PcurveGeometry, [f64; 2])>,
    admission: &'input10 mut FamilyEntityAdmission<'input8, 'input9>,
}

/// The model curve id of an edge record's boundary curve.
fn e5_edge_curve_id(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    record_id: u32,
    operation: &'static str,
) -> Result<CurveId, cadmpeg_core::CodecError> {
    crate::resource::compose_u32_id(
        ctx,
        &cadmpeg_ir::identity_namespace!("catia", "e5", "curve"),
        record_id,
        CurveId::mint,
        operation,
    )
}

fn emit_e5_curves_and_edges(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    inputs: EmitE5CurvesAndEdgesInputs<
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        impl cadmpeg_ir::annotations::AnnotationStorage,
    >,
) -> Result<(), cadmpeg_core::CodecError> {
    let EmitE5CurvesAndEdgesInputs {
        ir,
        annotations,
        topology,
        edge_curves,
        intersections,
        surface_curves,
        admission,
    } = inputs;

    for (&record_id, (geometry, _)) in
        ctx.admit_iter(edge_curves, "catia_e5_emitted_edge_curve_scan")?
    {
        let id = e5_edge_curve_id(ctx, record_id, "catia_e5_curve_record_id")?;
        annotate(
            ctx,
            annotations,
            &id,
            "e5_0d_03",
            0,
            "lifted_boundary_curve",
            Exactness::Derived,
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry",
            "catia_annotation_field",
        )?;
        admission.reserve_entity(&mut ir.model.curves, "catia_e5_model_curves")?;
        ir.model.curves.push(Curve {
            id,
            geometry: (geometry).try_clone_for_decode(ctx, "catia_e5_boundary_curve_copy")?,
            source_object: None,
        });
    }
    for (&record_id, context) in
        ctx.admit_iter(intersections, "catia_e5_emitted_intersection_scan")?
    {
        let curve = e5_edge_curve_id(ctx, record_id, "catia_e5_intersection_curve_id")?;
        let id = crate::resource::compose_u32_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "intersection"),
            record_id,
            ProceduralCurveId::mint,
            "catia_e5_intersection_id",
        )?;
        annotate(
            ctx,
            annotations,
            &id,
            "e5_0d_03",
            0,
            "c1_surface_intersection",
            Exactness::Derived,
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "curve",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "definition",
            "catia_annotation_field",
        )?;
        admission.charge()?;
        let _attached = ir.model.add_procedural_curve(
            ctx,
            &curve,
            ProceduralCurve::new(
                id,
                ProceduralCurveDefinition::Intersection {
                    context: crate::resource::copy_intcurve_support_context(
                        ctx,
                        context,
                        "catia_e5_intersection_context",
                    )?,
                    discontinuity_flag: false,
                    cache: None,
                },
            ),
        )?;
    }
    for (&record_id, (surface, pcurve, range)) in
        ctx.admit_iter(surface_curves, "catia_e5_emitted_surface_curve_scan")?
    {
        if ctx.contains_key_btree_map(intersections, &record_id, "catia_e5_intersection_plan")? {
            continue;
        }
        let curve = e5_edge_curve_id(ctx, record_id, "catia_e5_surface_curve_id")?;
        let id = crate::resource::compose_u32_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "surface-curve"),
            record_id,
            ProceduralCurveId::mint,
            "catia_e5_surface_curve_procedural_id",
        )?;
        annotate(
            ctx,
            annotations,
            &id,
            "e5_0d_03",
            0,
            "parametric_surface_curve",
            Exactness::Derived,
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "curve",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "definition",
            "catia_annotation_field",
        )?;
        admission.charge()?;
        let _attached = ir.model.add_procedural_curve(
            ctx,
            &curve,
            ProceduralCurve::new(
                id,
                ProceduralCurveDefinition::SurfaceCurve {
                    family: SurfaceCurveFamily::Parametric {
                        context: IntcurveSupportContext::try_new(
                            [
                                IntcurveSupportSide {
                                    surface: Some(surface.try_clone_for_decode(
                                        ctx,
                                        "catia_e5_surface_curve_support_id",
                                    )?),
                                    pcurve: Some(SupportPcurve::new(
                                        pcurve.try_clone_for_decode(
                                            ctx,
                                            "catia_e5_surface_curve_support_pcurve",
                                        )?,
                                        None,
                                    )),
                                },
                                IntcurveSupportSide {
                                    surface: None,
                                    pcurve: None,
                                },
                            ],
                            *range,
                            std::array::from_fn(|_| Vec::new()),
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                        tail: None,
                    },
                },
            ),
        )?;
    }
    for (&record_id, edge) in ctx.admit_iter(&topology.edges, "catia_e5_emitted_edge_scan")? {
        let id = e5_edge_id(ctx, record_id, "catia_e5_edge_record_id")?;
        annotate(
            ctx,
            annotations,
            &id,
            "e5_0d_03",
            0,
            "ff_edge_use",
            Exactness::ByteExact,
        )?;
        for field in ["start", "end"] {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &id,
                field,
                "catia_annotation_field",
            )?;
        }
        let curve_range = ctx
            .get_btree_map(edge_curves, &record_id, "catia_e5_edge_curve_plan")?
            .map(|(_, range)| *range);
        if curve_range.is_some() {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &id,
                "curve",
                "catia_annotation_field",
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &id,
                "param_range",
                "catia_annotation_field",
            )?;
        }
        let carrier = curve_range
            .map(|_| e5_edge_curve_id(ctx, record_id, "catia_e5_edge_carrier_id"))
            .transpose()?;
        admission.reserve_entity(&mut ir.model.edges, "catia_e5_model_edges")?;
        ir.model.edges.push(Edge {
            id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(carrier, curve_range)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            start: e5_vertex_id(
                ctx,
                &topology.vertex_refs,
                edge.start_vertex,
                "catia_e5_edge_start_id",
            )?,
            end: e5_vertex_id(
                ctx,
                &topology.vertex_refs,
                edge.end_vertex,
                "catia_e5_edge_end_id",
            )?,
            tolerance: None,
        });
    }
    Ok(())
}

/// Emits the surface pcurve layer.
fn emit_e5_pcurves(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, [f64; 2])>,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (&record_id, (geometry, range)) in
        ctx.admit_iter(pcurves, "catia_e5_emitted_pcurve_scan")?
    {
        let id = crate::resource::compose_u32_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "pcurve"),
            record_id,
            PcurveId::mint,
            "catia_e5_pcurve_id",
        )?;
        annotate(
            ctx,
            annotations,
            &id,
            "e5_0d_03",
            0,
            "surface_parameter_curve",
            Exactness::ByteExact,
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &id,
            "geometry",
            "catia_annotation_field",
        )?;
        admission.reserve_entity(&mut ir.model.pcurves, "catia_e5_model_pcurves")?;
        ir.model.pcurves.push(Pcurve {
            id,
            geometry: geometry.try_clone_for_decode(ctx, "catia_e5_model_pcurve_geometry")?,
            metadata: cadmpeg_ir::geometry::pcurve::PcurveMetadata::general(
                None,
                Some(
                    cadmpeg_ir::units::FiniteVector::new(*range)
                        .ok_or(
                            cadmpeg_ir::geometry::pcurve::PcurveMetadata::NON_FINITE_PARAMETER_RANGE,
                        )
                        .map_err(cadmpeg_core::CodecError::malformed)?,
                ),
                None,
            ),
        });
    }
    Ok(())
}

/// Emits the body/region/shell layer.
fn emit_e5_bodies(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    bodies: &[E5BodyPlan],
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (body_index, plan) in ctx
        .admit_iter(bodies, "catia_e5_emitted_body_plan_scan")?
        .enumerate()
    {
        let body_id = BodyId::mint(match plan.record_id {
            Some(record_id) => ctx.format_retained(
                format_args!("catia:e5:body#{record_id}"),
                "catia_e5_body_id",
            )?,
            None => ctx.format_retained(
                format_args!("catia:e5:body#inferred-{body_index}"),
                "catia_e5_body_id",
            )?,
        })
        .map_err(cadmpeg_core::CodecError::malformed)?;
        // The region ids are minted once as scratch; the body and each
        // region keep copies.
        let mut region_storage = ctx.reserve_scoped(0, "catia_e5_region_ids")?;
        let region_ids = region_storage.with_storage(|| {
            ctx.try_collect_vec(
                (0..plan.components.len()).map(|component| {
                    RegionId::mint(ctx.format_retained(
                        format_args!("catia:e5:region#{body_index}-{component}"),
                        "catia_e5_region_id",
                    )?)
                    .map_err(cadmpeg_core::CodecError::malformed)
                }),
                "catia_e5_region_ids",
            )
        })?;
        annotate(
            ctx,
            annotations,
            &body_id,
            "e5_0d_03",
            0,
            "01_body",
            if plan.record_id.is_some() {
                Exactness::ByteExact
            } else {
                Exactness::Inferred
            },
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &body_id,
            "kind",
            "catia_annotation_field",
        )?;
        crate::resource::derived_annotation(
            ctx,
            annotations,
            &body_id,
            "regions",
            "catia_annotation_field",
        )?;
        admission.reserve_entity(&mut ir.model.bodies, "catia_e5_model_bodies")?;
        ir.model.bodies.push(Body {
            id: body_id.try_clone_for_decode(ctx, "catia_e5_body_record_id")?,
            kind: plan.kind,
            regions: ctx.try_collect_vec(
                region_ids
                    .iter()
                    .map(|id| id.try_clone_for_decode(ctx, "catia_e5_body_region_id")),
                "catia_e5_body_regions",
            )?,
            transform: None,
            name: None,
            color: None,
            visible: None,
        });
        for (component, component_faces) in ctx
            .admit_iter(&plan.components, "catia_e5_emitted_shell_component_scan")?
            .enumerate()
        {
            let region_id =
                region_ids[component].try_clone_for_decode(ctx, "catia_e5_region_record_id")?;
            let shell_id = ShellId::mint(ctx.format_retained(
                format_args!("catia:e5:shell#{body_index}-{component}"),
                "catia_e5_shell_id",
            )?)
            .map_err(cadmpeg_core::CodecError::malformed)?;
            annotate(
                ctx,
                annotations,
                &region_id,
                "e5_0d_03",
                0,
                "derived_region",
                Exactness::Inferred,
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &region_id,
                "body",
                "catia_annotation_field",
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &region_id,
                "shells",
                "catia_annotation_field",
            )?;
            let mut shells = Vec::new();
            ctx.push_vec(
                &mut shells,
                shell_id.try_clone_for_decode(ctx, "catia_e5_region_shell_id")?,
                "catia_e5_region_shells",
            )?;
            admission.reserve_entity(&mut ir.model.regions, "catia_e5_model_regions")?;
            ir.model.regions.push(Region {
                id: region_id.try_clone_for_decode(ctx, "catia_e5_region_id_copy")?,
                body: body_id.try_clone_for_decode(ctx, "catia_e5_region_body_id")?,
                shells,
            });
            annotate(
                ctx,
                annotations,
                &shell_id,
                "e5_0d_03",
                0,
                "derived_shell",
                Exactness::Inferred,
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &shell_id,
                "region",
                "catia_annotation_field",
            )?;
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &shell_id,
                "faces",
                "catia_annotation_field",
            )?;
            admission.reserve_entity(&mut ir.model.shells, "catia_e5_model_shells")?;
            let mut face_ids = Vec::new();
            ctx.reserve_vec(
                &mut face_ids,
                component_faces.len(),
                "catia_e5_shell_face_ids",
            )?;
            for face in ctx.admit_iter(component_faces, "catia_e5_emitted_shell_face_scan")? {
                face_ids.push(crate::resource::compose_u32_id(
                    ctx,
                    &cadmpeg_ir::identity_namespace!("catia", "e5", "face"),
                    *face,
                    FaceId::mint,
                    "catia_e5_shell_face_id",
                )?);
            }
            ir.model.shells.push(
                match Shell::new(shell_id, region_id, face_ids, Vec::new(), Vec::new()) {
                    Ok(shell) => shell,
                    Err(_) => {
                        return Err(cadmpeg_core::CodecError::malformed(
                            "shell owns no topology",
                        ));
                    }
                },
            );
        }
    }
    Ok(())
}

/// Emits the face/loop/coedge layer and the radial-next fixup.
///
/// Returns `false` when the lowering plan is not total for a serialized loop
/// member.
struct EmitE5FacesLoopsCoedgesInputs<
    'input0,
    'input1,
    'input2,
    'input3,
    'input4,
    'input5,
    'input8,
    'input9,
    'input10,
    'input11,
    'input12,
    AnnotationAccount,
> {
    ir: &'input0 mut CadIr,
    annotations: &'input1 mut AnnotationBuilder<AnnotationAccount>,
    topology: &'input2 crate::families::e5::graph::E5Topology,
    surface_for_ref:
        &'input3 HashMap<u32, (SurfaceId, &'input4 crate::families::e5::records::E5Surface)>,
    face_shell: &'input5 HashMap<u32, ShellId>,
    boundary: &'input9 E5BoundaryPlan<'input8>,
    admission: &'input12 mut FamilyEntityAdmission<'input10, 'input11>,
}

fn emit_e5_faces_loops_coedges(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    inputs: EmitE5FacesLoopsCoedgesInputs<
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        '_,
        impl cadmpeg_ir::annotations::AnnotationStorage,
    >,
) -> Result<bool, cadmpeg_core::CodecError> {
    let EmitE5FacesLoopsCoedgesInputs {
        ir,
        annotations,
        topology,
        surface_for_ref,
        face_shell,
        boundary,
        admission,
    } = inputs;

    // The coedge occurrence groups are scratch for the radial-next fixup.
    let mut scratch = ctx.reserve_scoped(0, "catia_e5_radial_occurrences")?;
    let mut coedges_by_edge = BTreeMap::<u32, Vec<usize>>::new();
    for face_plan in ctx.admit_iter(&boundary.faces, "catia_e5_emitted_face_plan_scan")? {
        let face = face_plan.source;
        let face_id = crate::resource::compose_u32_id(
            ctx,
            &cadmpeg_ir::identity_namespace!("catia", "e5", "face"),
            face.record_id,
            FaceId::mint,
            "catia_e5_face_id",
        )?;
        let mut loop_ids = Vec::new();
        ctx.reserve_vec(&mut loop_ids, face.loops.len(), "catia_e5_face_loop_ids")?;
        for loop_ in ctx.admit_iter(&face.loops, "catia_e5_emitted_face_loop_id_scan")? {
            loop_ids.push(crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!("catia", "e5", "loop"),
                loop_.record_id,
                LoopId::mint,
                "catia_e5_face_loop_id",
            )?);
        }
        annotate(
            ctx,
            annotations,
            &face_id,
            "e5_0d_03",
            0,
            "00_advanced_face",
            Exactness::ByteExact,
        )?;
        for field in ["shell", "surface", "sense", "loops"] {
            crate::resource::derived_annotation(
                ctx,
                annotations,
                &face_id,
                field,
                "catia_annotation_field",
            )?;
        }
        let (Some(shell), Some((surface, _))) = (
            ctx.get_hash_map(face_shell, &face.record_id, "catia_e5_face_shells")?,
            ctx.get_hash_map(
                surface_for_ref,
                &face.surface,
                "catia_e5_transfer_surface_refs",
            )?,
        ) else {
            return Ok(false);
        };
        admission.reserve_entity(&mut ir.model.faces, "catia_e5_model_faces")?;
        ir.model.faces.push(Face {
            id: face_id.try_clone_for_decode(ctx, "catia_e5_face_record_id")?,
            shell: shell.try_clone_for_decode(ctx, "catia_e5_face_shell_id")?,
            surface: surface.try_clone_for_decode(ctx, "catia_e5_face_surface_id")?,
            sense: if face.trailer_sign == crate::families::e5::graph::Sign::Positive {
                Sense::Forward
            } else {
                Sense::Reversed
            },
            loops: match loop_ids.split_first() {
                // The source states the outer boundary first.
                Some((outer, inner)) => cadmpeg_ir::topology::FaceLoops::classified(
                    outer.try_clone_for_decode(ctx, "catia_e5_outer_loop_id")?,
                    ctx.try_collect_vec(
                        inner
                            .iter()
                            .map(|id| id.try_clone_for_decode(ctx, "catia_e5_inner_loop_id")),
                        "catia_e5_inner_loop_ids",
                    )?,
                ),
                None => cadmpeg_ir::topology::FaceLoops::unspecified(Vec::new()),
            },
            name: None,
            color: None,
            tolerance: None,
        });

        for loop_plan in ctx.admit_iter(&face_plan.loops, "catia_e5_emitted_loop_plan_scan")? {
            let loop_ = loop_plan.source;
            let loop_id = crate::resource::compose_u32_id(
                ctx,
                &cadmpeg_ir::identity_namespace!("catia", "e5", "loop"),
                loop_.record_id,
                LoopId::mint,
                "catia_e5_loop_id",
            )?;
            let members = &loop_plan.members;
            let mut coedge_ids = Vec::new();
            let mut vertex_uses = Vec::new();
            for member in ctx.admit_iter(members, "catia_e5_emitted_loop_member_scan")? {
                ctx.push_vec(
                    &mut coedge_ids,
                    member
                        .id
                        .try_clone_for_decode(ctx, "catia_e5_loop_coedge_id")?,
                    "catia_e5_loop_coedge_ids",
                )?;
                let edge_ref = member.source.edge_use;
                let Some(edge) =
                    ctx.get_btree_map(&topology.edges, &edge_ref, "catia_e5_coedge_edge_lookup")?
                else {
                    return Ok(false);
                };
                let endpoint_ref = if member.orientation.reversed {
                    edge.start_vertex
                } else {
                    edge.end_vertex
                };
                if e5_vertex_ordinal(ctx, &topology.vertex_refs, endpoint_ref)?.is_none() {
                    return Ok(false);
                }
                let vertex_use = AnchoredVertexUse {
                    vertex: e5_vertex_id(
                        ctx,
                        &topology.vertex_refs,
                        endpoint_ref,
                        "catia_e5_vertex_use_id",
                    )?,
                    after: member
                        .id
                        .try_clone_for_decode(ctx, "catia_e5_vertex_use_coedge_id")?,
                    pcurves: Vec::new(),
                };
                ctx.push_vec(&mut vertex_uses, vertex_use, "catia_e5_loop_vertex_uses")?;
            }
            annotate(
                ctx,
                annotations,
                &loop_id,
                "e5_0d_03",
                0,
                "09_loop",
                Exactness::ByteExact,
            )?;
            for field in ["face", "coedges", "vertex_uses"] {
                crate::resource::derived_annotation(
                    ctx,
                    annotations,
                    &loop_id,
                    field,
                    "catia_annotation_field",
                )?;
            }
            let Ok(ring) = cadmpeg_ir::topology::LoopRing::new(ctx, coedge_ids, vertex_uses)
                .map_err(cadmpeg_core::CodecError::from)?
            else {
                return Ok(false);
            };
            admission.reserve_entity(&mut ir.model.loops, "catia_e5_model_loops")?;
            ir.model.loops.push(Loop {
                id: loop_id.try_clone_for_decode(ctx, "catia_e5_loop_record_id")?,
                face: face_id.try_clone_for_decode(ctx, "catia_e5_loop_face_id")?,
                boundary: cadmpeg_ir::topology::LoopBoundary::Ring(ring),
            });
            for member in ctx.admit_iter(members, "catia_e5_emitted_coedge_member_scan")? {
                let index = member.orientation.serialized_index;
                let edge_ref = member.source.edge_use;
                let pcurve_ref = member.source.pcurve;
                let Some(&pcurve_reversed) = ctx.get_btree_map(
                    &boundary.pcurve_use_reversed,
                    &(loop_.record_id, index),
                    "catia_e5_boundary_occurrence_senses",
                )?
                else {
                    return Ok(false);
                };
                let Some((_, range)) =
                    ctx.get_btree_map(&boundary.pcurves, &pcurve_ref, "catia_e5_pcurve_plan")?
                else {
                    return Ok(false);
                };
                let pcurve_parameter_range =
                    (member.orientation.reversed ^ pcurve_reversed).then_some([range[1], range[0]]);
                let id = member.id.try_clone_for_decode(ctx, "catia_e5_coedge_id")?;
                annotate(
                    ctx,
                    annotations,
                    &id,
                    "e5_0d_03",
                    0,
                    "serialized_loop_member",
                    Exactness::ByteExact,
                )?;
                for field in ["owner_loop", "edge", "sense", "pcurves"] {
                    crate::resource::derived_annotation(
                        ctx,
                        annotations,
                        &id,
                        field,
                        "catia_annotation_field",
                    )?;
                }
                let arena_index = ir.model.coedges.len();
                scratch.with_storage(|| {
                    ctx.push_btree_group(
                        &mut coedges_by_edge,
                        edge_ref,
                        arena_index,
                        "catia_e5_radial_edge_keys",
                        "catia_e5_radial_occurrences",
                    )
                })?;
                let Ok(parameter_range) = pcurve_parameter_range
                    .map(cadmpeg_ir::geometry::DirectedParameterRange::new)
                    .transpose()
                else {
                    return Ok(false);
                };
                let mut pcurves = Vec::new();
                ctx.push_vec(
                    &mut pcurves,
                    cadmpeg_ir::topology::PcurveUse {
                        pcurve: crate::resource::compose_u32_id(
                            ctx,
                            &cadmpeg_ir::identity_namespace!("catia", "e5", "pcurve"),
                            pcurve_ref,
                            PcurveId::mint,
                            "catia_e5_coedge_pcurve_id",
                        )?,
                        isoparametric: None,
                        parameter_range,
                    },
                    "catia_e5_coedge_pcurve_uses",
                )?;
                admission.reserve_entity(&mut ir.model.coedges, "catia_e5_model_coedges")?;
                ir.model.coedges.push(Coedge {
                    id: id.try_clone_for_decode(ctx, "catia_e5_coedge_record_id")?,
                    owner_loop: loop_id.try_clone_for_decode(ctx, "catia_e5_coedge_loop_id")?,
                    edge: e5_edge_id(ctx, edge_ref, "catia_e5_coedge_edge_id")?,
                    radial_next: id,
                    sense: if member.orientation.reversed {
                        Sense::Reversed
                    } else {
                        Sense::Forward
                    },
                    pcurves,
                    use_curve: None,
                });
            }
        }
    }
    for (_, occurrences) in
        ctx.admit_iter(&coedges_by_edge, "catia_e5_radial_occurrence_group_scan")?
    {
        for (position, &arena_index) in ctx
            .admit_iter(occurrences, "catia_e5_radial_occurrence_scan")?
            .enumerate()
        {
            let radial = occurrences[(position + 1) % occurrences.len()];
            let next = ir.model.coedges[radial]
                .id
                .try_clone_for_decode(ctx, "catia_e5_coedge_radial_next_id")?;
            ir.model.coedges[arena_index].radial_next = next;
        }
    }
    Ok(true)
}

fn e5_stored_pcurve_reversed(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::families::e5::graph::E5Topology,
    edge_ref: u32,
    pcurve_ref: u32,
    native_range: [f64; 2],
) -> Result<Option<bool>, cadmpeg_core::CodecError> {
    Ok(topology
        .edge_representation_parameters(ctx, edge_ref, pcurve_ref)?
        .and_then(|parameters| {
            parameter_ranges_reversed(parameters.map(FiniteReal::get), native_range)
        }))
}

fn unique_endpoint_direction(forward_error: f64, reverse_error: f64) -> Option<bool> {
    match (
        forward_error.is_finite() && forward_error <= E5_ENDPOINT_MATCH_TOLERANCE,
        reverse_error.is_finite() && reverse_error <= E5_ENDPOINT_MATCH_TOLERANCE,
    ) {
        (true, false) => Some(false),
        (false, true) => Some(true),
        (true, true) | (false, false) => None,
    }
}

fn parameter_ranges_reversed(parameters: [f64; 2], native_range: [f64; 2]) -> Option<bool> {
    if !parameters
        .into_iter()
        .chain(native_range)
        .all(f64::is_finite)
        || parameters[0] == parameters[1]
        || native_range[0] == native_range[1]
    {
        return None;
    }
    Some((parameters[1] < parameters[0]) != (native_range[1] < native_range[0]))
}

fn e5_pcurve_on_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &crate::families::e5::graph::E5Pcurve,
    decoded_surface: &crate::families::e5::records::E5Surface,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> E5PcurveLiftOutput {
    let surface = &decoded_surface.geometry;
    match pcurve {
        crate::families::e5::graph::E5Pcurve::Line {
            origin: raw_origin,
            direction,
            range,
            ..
        } => {
            let range = range.map(FiniteReal::get);
            let origin = e5_surface_uv(decoded_surface, *raw_origin);
            let direction = Point2::new(
                direction[0].get() * decoded_surface.uv_scale[0].get(),
                direction[1].get() * decoded_surface.uv_scale[1].get(),
            );
            if !origin.is_finite() || !direction.is_finite() {
                return Ok(None);
            }
            let uv = range.map(|parameter| {
                Point2::new(
                    origin.u + parameter * direction.u,
                    origin.v + parameter * direction.v,
                )
            });
            if !uv.iter().copied().all(|point| point.is_finite()) {
                return Ok(None);
            }
            let Some(start) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv[0].u, uv[0].v),
                )?)?
            else {
                return Ok(None);
            };
            let Some(end) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, surface, uv[1].u, uv[1].v),
                )?)?
            else {
                return Ok(None);
            };
            let Ok(line) = cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(origin, direction)
            else {
                return Ok(None);
            };
            Ok(Some((
                PcurveGeometry::Line(line),
                range,
                [start.get(), end.get()],
            )))
        }
        crate::families::e5::graph::E5Pcurve::Circle {
            center,
            radius,
            range,
            ..
        } => {
            let (center, radius) = (center.map(FiniteReal::get), radius.get());
            let angular_range = ordered_range([range[0].get() / radius, range[1].get() / radius]);
            if !angular_range.into_iter().all(f64::is_finite) {
                return Ok(None);
            }
            let Some(geometry) = rational_pcurve_arc(
                ctx,
                center,
                radius,
                angular_range,
                refusal,
                "e5 arc pcurve record",
            )?
            else {
                return Ok(None);
            };
            {
                let PcurveGeometry::Nurbs { mut nurbs } = geometry else {
                    return Ok(None);
                };
                let scale = decoded_surface.uv_scale.map(FiniteReal::get);
                if nurbs
                    .try_map_control_points(
                        |_, point| {
                            let point = point.get();
                            FinitePoint2::new(Point2::new(point.u * scale[0], point.v * scale[1]))
                                .ok_or(())
                        },
                        ctx,
                    )?
                    .is_err()
                {
                    return Ok(None);
                }
                let geometry = PcurveGeometry::Nurbs { nurbs };
                let start_angle = angular_range[0];
                let end_angle = angular_range[1];
                let Some(start) =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::surface_point(
                            ctx,
                            surface,
                            (center[0] + radius * start_angle.cos()) * scale[0],
                            (center[1] + radius * start_angle.sin()) * scale[1],
                        ),
                    )?)?
                else {
                    return Ok(None);
                };
                let Some(end) =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::surface_point(
                            ctx,
                            surface,
                            (center[0] + radius * end_angle.cos()) * scale[0],
                            (center[1] + radius * end_angle.sin()) * scale[1],
                        ),
                    )?)?
                else {
                    return Ok(None);
                };
                Ok(Some((geometry, angular_range, [start.get(), end.get()])))
            }
        }
        crate::families::e5::graph::E5Pcurve::Jet { sites, range, .. } => {
            let scale = decoded_surface.uv_scale.map(FiniteReal::get);
            let (mut knots, _knots_reservation) =
                ctx.temporary_vec(sites.len(), "catia E5 pcurve jet knots")?;
            let (mut points, _points_reservation) =
                ctx.temporary_vec(sites.len(), "catia E5 pcurve jet points")?;
            let (mut first_derivatives, _first_reservation) =
                ctx.temporary_vec(sites.len(), "catia E5 pcurve first jets")?;
            let (mut second_derivatives, _second_reservation) =
                ctx.temporary_vec(sites.len(), "catia E5 pcurve second jets")?;
            let scaled =
                |values: [FiniteReal; 2]| [values[0].get() * scale[0], values[1].get() * scale[1]];
            let finite = |values: [f64; 2]| values[0].is_finite() && values[1].is_finite();
            for site in ctx.admit_iter(sites, "catia_e5_jet_site_scan")? {
                let point = scaled(site.point);
                let first = scaled(site.first_derivatives);
                let second = scaled(site.second_derivatives);
                if !finite(point) || !finite(first) || !finite(second) {
                    return Ok(None);
                }
                knots.push(site.knot.get());
                points.push(point);
                first_derivatives.push(first);
                second_derivatives.push(second);
            }
            let Some(geometry) = quintic_jet_pcurve(
                ctx,
                crate::families::e5::graph::E5Pcurve::JET_DEGREE,
                &knots,
                &points,
                (&first_derivatives, &second_derivatives),
                refusal,
                format_args!(
                    "e5 quintic-jet pcurve on surface record {} at byte {}",
                    decoded_surface.record_id, decoded_surface.pos
                ),
            )?
            else {
                return Ok(None);
            };
            let (Some(first), Some(last)) = (points.first(), points.last()) else {
                return Ok(None);
            };
            let Some(start) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, surface, first[0], first[1]),
                )?)?
            else {
                return Ok(None);
            };
            let Some(end) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::surface_point(ctx, surface, last[0], last[1]),
                )?)?
            else {
                return Ok(None);
            };
            Ok(Some((
                geometry,
                range.map(FiniteReal::get),
                [start.get(), end.get()],
            )))
        }
        crate::families::e5::graph::E5Pcurve::Nurbs {
            degree,
            knots,
            control_points,
            range,
            ..
        } => {
            let scale = decoded_surface.uv_scale.map(FiniteReal::get);
            if !scale.into_iter().all(|value| value != 0.0) {
                return Ok(None);
            }
            let Some(scaled_points) = ctx.collect_options(
                control_points.iter().map(|[u, v]| {
                    let point = Point2::new(u.get() * scale[0], v.get() * scale[1]);
                    point.is_finite().then_some(point)
                }),
                "catia E5 NURBS pcurve points",
            )?
            else {
                return Ok(None);
            };
            let knot_values = ctx.collect_vec(
                knots.iter().copied().map(FiniteReal::get),
                "catia E5 NURBS pcurve knots",
            )?;
            let Some(nurbs) = crate::nurbs::note_refusal(
                ctx,
                PcurveNurbs::from_lanes(ctx, *degree, knot_values, scaled_points, None, false)?,
                refusal,
                format_args!(
                    "e5 NURBS pcurve on surface record {} at byte {}",
                    decoded_surface.record_id, decoded_surface.pos
                ),
            )?
            else {
                return Ok(None);
            };
            let geometry = PcurveGeometry::Nurbs { nurbs };
            {
                let range = range.map(FiniteReal::get);
                let Some(start_uv) =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, range[0]),
                    )?)?
                else {
                    return Ok(None);
                };
                let Some(end_uv) =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::pcurve_uv(ctx, &geometry, range[1]),
                    )?)?
                else {
                    return Ok(None);
                };
                let Some(start) =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::surface_point(
                            ctx, surface, start_uv.u, start_uv.v,
                        ),
                    )?)?
                else {
                    return Ok(None);
                };
                let Some(end) =
                    cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                        cadmpeg_ir::eval::decode::surface_point(ctx, surface, end_uv.u, end_uv.v),
                    )?)?
                else {
                    return Ok(None);
                };
                Ok(Some((geometry, range, [start.get(), end.get()])))
            }
        }
    }
}

fn e5_lift_plane_nurbs(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    plane: &cadmpeg_ir::geometry::analytic::PlaneSurface,
    nurbs: &PcurveNurbs,
    surface_record_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<NurbsCurve>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoles3, WeightedPole3};
    use cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles;

    let origin = plane.origin().get();
    let normal = plane.frame().axis().as_raw();
    let u_axis = plane.frame().reference().as_raw();
    let v_axis = (*normal).cross(*u_axis);
    if !v_axis.is_finite() {
        return Ok(None);
    }
    let lift = |point: FinitePoint2| {
        let point = point.get();
        FinitePoint3::new(
            origin
                .translated(*u_axis, point.u)
                .translated(v_axis, point.v),
        )
    };
    let operation = "catia_e5_boundary_lifted_poles";
    let poles = match nurbs.pole_rows() {
        PcurveNurbsPoles::Polynomial { points } => {
            let Some(points) = ctx.collect_options(points.iter().copied().map(lift), operation)?
            else {
                return Ok(None);
            };
            NurbsPoles3::Polynomial { points }
        }
        PcurveNurbsPoles::Rational { points } => {
            let Some(points) = ctx.collect_options(
                points.iter().map(|pole| {
                    Some(WeightedPole3 {
                        point: lift(pole.point)?,
                        weight: pole.weight,
                    })
                }),
                operation,
            )?
            else {
                return Ok(None);
            };
            NurbsPoles3::Rational { points }
        }
    };
    let knots = nurbs
        .knots()
        .try_clone_for_decode(ctx, "catia_e5_boundary_lifted_knots")?;
    crate::nurbs::note_refusal(
        ctx,
        NurbsCurve::new(ctx, nurbs.degree(), knots, poles, nurbs.periodic())?,
        refusal,
        format_args!(
            "e5 boundary curve lifted from the pcurve on surface record {surface_record_id}"
        ),
    )
}

fn e5_boundary_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &SurfaceGeometry,
    native_pcurve: &crate::families::e5::graph::E5Pcurve,
    pcurve: &PcurveGeometry,
    (range, endpoints): ([f64; 2], [Point3; 2]),
    uv_scale: [FiniteReal; 2],
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<(CurveGeometry, [f64; 2])>, cadmpeg_core::CodecError> {
    let uv_scale = uv_scale.map(FiniteReal::get);
    if !uv_scale.into_iter().all(|value| value != 0.0)
        || !range.into_iter().all(f64::is_finite)
        || !endpoints.iter().copied().all(|point| point.is_finite())
    {
        return Ok(None);
    }
    if let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
        crate::families::e5::graph::E5Pcurve::Circle { center, radius, .. },
    ) = (surface, native_pcurve)
    {
        let origin = plane_surface.origin().get();
        let normal = plane_surface.frame().axis().as_raw();
        let u_axis = plane_surface.frame().reference().as_raw();
        let v_axis = (*normal).cross(*u_axis);
        let center = origin
            .translated(*u_axis, center[0].get() * uv_scale[0])
            .translated(v_axis, center[1].get() * uv_scale[1]);
        if !center.is_finite() || !v_axis.is_finite() {
            return Ok(None);
        }
        let (Ok(circle), Some(range)) = (
            cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
                center,
                *normal,
                u_axis.scale(uv_scale[0]),
                radius.get(),
            ),
            crate::nurbs::canonical_periodic_range(range),
        ) else {
            return Ok(None);
        };
        return Ok(Some((
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)),
            range,
        )));
    }
    if let (
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)),
        crate::families::e5::graph::E5Pcurve::Jet { .. }
        | crate::families::e5::graph::E5Pcurve::Nurbs { .. },
        PcurveGeometry::Nurbs { nurbs },
    ) = (surface, native_pcurve, pcurve)
    {
        let Some(nurbs) = e5_lift_plane_nurbs(
            ctx,
            plane_surface,
            nurbs,
            native_pcurve.surface_record_id(),
            refusal,
        )?
        else {
            return Ok(None);
        };
        return Ok(Some((
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)),
            range,
        )));
    }
    let PcurveGeometry::Line(line_pcurve) = pcurve else {
        return Ok(None);
    };
    let direction = line_pcurve.direction().as_raw();
    let Some(start_uv) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, range[0]),
        )?)?
    else {
        return Ok(None);
    };
    let Some(end_uv) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, range[1]),
        )?)?
    else {
        return Ok(None);
    };

    let circle = match e5_isoparametric_direction(*direction) {
        Some(E5IsoparametricDirection::ConstantV) => {
            e5_constant_v_circle(surface, start_uv.as_raw().v)
        }
        Some(E5IsoparametricDirection::ConstantU) => {
            e5_constant_u_circle(surface, start_uv.as_raw().u)
        }
        None => None,
    };
    if let Some((center, radius, axis)) = circle {
        if !center.is_finite() || !axis.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return Ok(None);
        }
        let Some(span_direction) = FinitePoint2::new(Point2::new(
            end_uv.as_raw().u - start_uv.as_raw().u,
            end_uv.as_raw().v - start_uv.as_raw().v,
        )) else {
            return Ok(None);
        };
        let mut choices = [None, None];
        for (index, axis) in [axis, axis.scale(-1.0)].into_iter().enumerate() {
            let ref_direction = cadmpeg_ir::geometry::derive_reference_direction(axis);
            let Some(range) = circle_parameter_range_from_surface_branch(
                ctx,
                crate::assemble::CircleParameterRangeFromSurfaceBranchInputs {
                    surface,
                    center,
                    radius,
                    axis,
                    ref_direction,
                    start: endpoints[0],
                    end: endpoints[1],
                    pcurve_origin: start_uv,
                    pcurve_direction: span_direction,
                },
            )?
            else {
                continue;
            };
            let Some(range) = crate::nurbs::canonical_periodic_range(range) else {
                continue;
            };
            choices[index] = Some((axis, ref_direction, range));
        }
        let [first, second] = choices;
        let ((Some((axis, ref_direction, curve_range)), None)
        | (None, Some((axis, ref_direction, curve_range)))) = (first, second)
        else {
            return Ok(None);
        };
        let Ok(circle) = cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            center,
            axis,
            ref_direction,
            radius,
        ) else {
            return Ok(None);
        };
        return Ok(Some((
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)),
            curve_range,
        )));
    }

    if !(matches!(
        surface,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(_))
    ) || (direction.u == 0.0
        && matches!(
            surface,
            SurfaceGeometry::Solved(
                SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_)
            )
        )))
    {
        return Ok(None);
    }
    let delta = endpoints[1].vector_from(endpoints[0]);
    let length = delta.x.hypot(delta.y).hypot(delta.z);
    if !length.is_finite() || length <= 0.0 {
        return Ok(None);
    }
    let direction = Vector3::new(delta.x / length, delta.y / length, delta.z / length);
    let Ok(line) = cadmpeg_ir::geometry::analytic::LineCurve::try_new(endpoints[0], direction)
    else {
        return Ok(None);
    };
    Ok(Some((
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line)),
        [0.0, length],
    )))
}

#[cfg(test)]
fn e5_occurrence_intersection_context(
    sides: &[(SurfaceId, PcurveGeometry, [f64; 2])],
) -> Option<IntcurveSupportContext> {
    let [left, right] = sides else {
        return None;
    };
    let tolerance = parameter_range_agreement_tolerance(left.2, right.2)?;
    if (left.2[0] - right.2[0]).abs() > tolerance || (left.2[1] - right.2[1]).abs() > tolerance {
        return None;
    }
    IntcurveSupportContext::try_new(
        [left, right].map(|side| IntcurveSupportSide {
            surface: Some(side.0.clone()),
            pcurve: Some(SupportPcurve::new(side.1.clone(), None)),
        }),
        left.2,
        std::array::from_fn(|_| Vec::new()),
    )
    .ok()
}

fn e5_support_occurrence_intersection_context(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    support_range: [f64; 2],
    solved_range: [f64; 2],
    sides: &[E5OccurrenceIntersectionSide],
) -> Result<Option<IntcurveSupportContext>, cadmpeg_core::CodecError> {
    let [left, right] = sides else {
        return Ok(None);
    };
    if left.surface == right.surface {
        return Ok(None);
    }
    if !e5_parameter_range_is_valid(support_range)
        || !e5_parameter_range_is_valid(solved_range)
        || !e5_parameter_range_is_valid(left.pcurve_range)
        || !e5_parameter_range_is_valid(right.pcurve_range)
    {
        return Ok(None);
    }
    let [left_side, right_side] = [left, right].map(|side| {
        Ok::<_, cadmpeg_core::CodecError>(IntcurveSupportSide {
            surface: Some(
                side.surface
                    .try_clone_for_decode(ctx, "catia_e5_occurrence_context_surface_id")?,
            ),
            pcurve: Some(SupportPcurve::new(
                side.pcurve
                    .try_clone_for_decode(ctx, "catia_e5_occurrence_context_pcurve")?,
                DirectedParameterRange::new(side.pcurve_range).ok(),
            )),
        })
    });
    Ok(IntcurveSupportContext::try_new(
        [left_side?, right_side?],
        solved_range,
        std::array::from_fn(|_| Vec::new()),
    )
    .ok())
}

fn e5_occurrence_intersection_cache<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    sides: &'a [E5OccurrenceIntersectionSide],
) -> Result<Option<(&'a CurveGeometry, [f64; 2])>, cadmpeg_core::decode::ResourceLimit> {
    let [left, right] = sides else {
        return Ok(None);
    };
    let (Some((left_curve, left_range)), Some((right_curve, right_range))) =
        (&left.curve, &right.curve)
    else {
        return Ok(None);
    };
    if equivalent_e5_curve_carriers(left_curve, right_curve)
        && parameter_span_agreement(*left_range, *right_range).is_some()
    {
        return Ok(Some((left_curve, *left_range)));
    }
    if e5_circle_carriers_have_same_ordered_sweep(
        ctx,
        left_curve,
        *left_range,
        right_curve,
        *right_range,
    )? {
        return Ok(Some((left_curve, *left_range)));
    }
    Ok(
        match (
            is_exact_e5_analytic_curve(left_curve),
            is_exact_e5_analytic_curve(right_curve),
            is_e5_nurbs_curve(right_curve),
            is_e5_nurbs_curve(left_curve),
        ) {
            (true, false, true, _) => Some((left_curve, *left_range)),
            (false, true, _, true) => Some((right_curve, *right_range)),
            _ => None,
        },
    )
}

fn is_exact_e5_analytic_curve(curve: &CurveGeometry) -> bool {
    matches!(
        curve,
        CurveGeometry::Solved(SolvedCurveGeometry::Line(_) | SolvedCurveGeometry::Circle(_))
    )
}

fn is_e5_nurbs_curve(curve: &CurveGeometry) -> bool {
    matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_)))
}

fn e5_parameter_range_is_valid(range: [f64; 2]) -> bool {
    range.into_iter().all(f64::is_finite) && range[0] != range[1]
}

fn parameter_span_magnitudes(left: [f64; 2], right: [f64; 2]) -> (f64, f64, f64) {
    let left_span = (left[1] - left[0]).abs();
    let right_span = (right[1] - right[0]).abs();
    if left_span.is_finite() && right_span.is_finite() {
        (left_span, right_span, 1.0)
    } else {
        (
            (left[1] * 0.5 - left[0] * 0.5).abs(),
            (right[1] * 0.5 - right[0] * 0.5).abs(),
            2.0,
        )
    }
}

fn parameter_span_agreement(left: [f64; 2], right: [f64; 2]) -> Option<f64> {
    if !e5_parameter_range_is_valid(left) || !e5_parameter_range_is_valid(right) {
        return None;
    }
    let (left_span, right_span, scale_factor) = parameter_span_magnitudes(left, right);
    let parameter_scale = left_span.max(right_span);
    if parameter_scale == 0.0
        || (left_span - right_span).abs() > E5_PARAMETER_RELATIVE_TOLERANCE * parameter_scale
    {
        return None;
    }
    Some(E5_PARAMETER_RELATIVE_TOLERANCE * parameter_scale * scale_factor)
}

fn parameter_range_agreement_tolerance(left: [f64; 2], right: [f64; 2]) -> Option<f64> {
    if !left.into_iter().chain(right).all(f64::is_finite) {
        return None;
    }
    let (left_span, right_span, scale_factor) = parameter_span_magnitudes(left, right);
    let parameter_scale = left_span.max(right_span);
    if parameter_scale == 0.0
        || (left_span - right_span).abs() > E5_PARAMETER_RELATIVE_TOLERANCE * parameter_scale
        || (left[0] - right[0]).abs()
            > E5_PARAMETER_RELATIVE_TOLERANCE * parameter_scale * scale_factor
        || (left[1] - right[1]).abs()
            > E5_PARAMETER_RELATIVE_TOLERANCE * parameter_scale * scale_factor
    {
        return None;
    }
    Some(E5_PARAMETER_RELATIVE_TOLERANCE * parameter_scale * scale_factor)
}

fn e5_circle_carriers_have_same_ordered_sweep(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    left: &CurveGeometry,
    left_range: [f64; 2],
    right: &CurveGeometry,
    right_range: [f64; 2],
) -> Result<bool, cadmpeg_core::decode::ResourceLimit> {
    let (
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve_2)),
    ) = (left, right)
    else {
        return Ok(false);
    };
    let left_center = circle_curve.center().get();
    let left_axis = circle_curve.frame().axis().as_raw();
    let left_radius = circle_curve.radius().get();
    let right_center = circle_curve_2.center().get();
    let right_axis = circle_curve_2.frame().axis().as_raw();
    let right_radius = circle_curve_2.radius().get();
    if (left_center).distance(right_center) > E5_ENDPOINT_MATCH_TOLERANCE
        || (left_radius - right_radius).abs() > E5_ENDPOINT_MATCH_TOLERANCE
        || (*left_axis).dot(*right_axis) < 1.0 - E5_CARRIER_AXIS_COSINE_TOLERANCE
    {
        return Ok(false);
    }
    let left_span = left_range[1] - left_range[0];
    let right_span = right_range[1] - right_range[0];
    if left_span.signum() != right_span.signum()
        || parameter_span_agreement(left_range, right_range).is_none()
    {
        return Ok(false);
    }
    let Some(left_start) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, left, left_range[0]),
        )?)?
    else {
        return Ok(false);
    };
    let Some(left_end) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, left, left_range[1]),
        )?)?
    else {
        return Ok(false);
    };
    let Some(right_start) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, right, right_range[0]),
        )?)?
    else {
        return Ok(false);
    };
    let Some(right_end) =
        cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
            cadmpeg_ir::eval::decode::curve_point(ctx, right, right_range[1]),
        )?)?
    else {
        return Ok(false);
    };
    Ok(
        left_start.distance(right_start.get()) <= E5_ENDPOINT_MATCH_TOLERANCE
            && left_end.distance(right_end.get()) <= E5_ENDPOINT_MATCH_TOLERANCE,
    )
}

fn equivalent_e5_curve_carriers(left: &CurveGeometry, right: &CurveGeometry) -> bool {
    match (left, right) {
        (
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)),
            CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve_2)),
        ) => {
            let left_origin = line_curve.origin().get();
            let left_direction = *line_curve.direction().as_raw();
            let right_origin = line_curve_2.origin().get();
            let right_direction = *line_curve_2.direction().as_raw();
            left_origin.distance(right_origin) <= 2e-3
                && left_direction.dot(right_direction) >= 1.0 - EPS_E5_DECODE_GEOMETRY
        }
        (
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)),
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve_2)),
        ) => {
            let left_center = circle_curve.center().get();
            let left_axis = circle_curve.frame().axis().as_raw();
            let left_ref_direction = circle_curve.frame().reference().as_raw();
            let left_radius = circle_curve.radius().get();
            let right_center = circle_curve_2.center().get();
            let right_axis = circle_curve_2.frame().axis().as_raw();
            let right_ref_direction = circle_curve_2.frame().reference().as_raw();
            let right_radius = circle_curve_2.radius().get();
            (left_center).distance(right_center) <= 2e-3
                && (left_radius - right_radius).abs() <= 2e-3
                && (*left_axis).dot(*right_axis) >= 1.0 - EPS_E5_DECODE_GEOMETRY
                && (*left_ref_direction).dot(*right_ref_direction) >= 1.0 - EPS_E5_DECODE_GEOMETRY
        }
        (
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(left)),
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(right)),
        ) => left == right,
        _ => false,
    }
}

fn e5_constant_v_circle(surface: &SurfaceGeometry, v: f64) -> Option<(Point3, f64, Vector3)> {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
            let origin = cylinder_surface.origin().get();
            let axis = cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            Some((origin.translated(*axis, v), radius, *axis))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
            let origin = cone_surface.origin().get();
            let axis = cone_surface.frame().axis().as_raw();
            let radius = cone_surface.radius().get();
            let half_angle = cone_surface.half_angle().get();
            Some((
                origin.translated(*axis, v),
                (radius + v * half_angle.tan()).abs(),
                *axis,
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let radius = sphere_surface.radius().get();
            Some((
                center.translated(*axis, radius * v.sin()),
                radius * v.cos().abs(),
                *axis,
            ))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            Some((
                center.translated(*axis, minor_radius * v.sin()),
                (major_radius + minor_radius * v.cos()).abs(),
                *axis,
            ))
        }
        _ => None,
    }
}

fn e5_constant_u_circle(surface: &SurfaceGeometry, u: f64) -> Option<(Point3, f64, Vector3)> {
    match surface {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
            let center = sphere_surface.center().get();
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            let tangent = (*axis).cross(*ref_direction);
            let radial = (*ref_direction).scale(u.cos()) + tangent.scale(u.sin());
            Some((center, radius, (*axis).cross(radial)))
        }
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
            let center = torus_surface.center().get();
            let axis = torus_surface.frame().axis().as_raw();
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            let tangent = (*axis).cross(*ref_direction);
            let radial = (*ref_direction).scale(u.cos()) + tangent.scale(u.sin());
            Some((
                center.translated(radial, major_radius),
                minor_radius,
                (*axis).cross(radial),
            ))
        }
        _ => None,
    }
}

fn e5_surface_uv(
    surface: &crate::families::e5::records::E5Surface,
    raw: [FiniteReal; 2],
) -> Point2 {
    Point2::new(
        raw[0].get() * surface.uv_scale[0].get(),
        raw[1].get() * surface.uv_scale[1].get(),
    )
}

struct E5BodyPlan {
    record_id: Option<u32>,
    kind: BodyKind,
    components: Vec<Vec<u32>>,
}

fn e5_ownership_plan(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &crate::families::e5::graph::E5Topology,
    body_faces: &[(Option<u32>, Vec<u32>)],
) -> Result<Option<Vec<E5BodyPlan>>, cadmpeg_core::CodecError> {
    if body_faces.is_empty()
        || ctx.any_by(
            body_faces,
            |(_, faces)| Ok(faces.is_empty()),
            "catia_e5_body_face_group_validation_scan",
        )?
    {
        return Ok(None);
    }
    let mut body_by_face = HashMap::new();
    for (body, (_, faces)) in ctx
        .admit_iter(body_faces, "catia_e5_body_face_group_scan")?
        .enumerate()
    {
        for face in ctx.admit_iter(faces, "catia_e5_body_face_scan")? {
            if ctx
                .insert_hash_map(&mut body_by_face, *face, body, "catia_e5_body_faces")?
                .is_some()
            {
                return Ok(None);
            }
        }
    }
    // Each body's topology faces, and its edge-use counts in edge order.
    let mut body_topology_faces =
        ctx.collect_indexed_vec(body_faces.len(), "catia_e5_body_topology_faces", |_| {
            Ok(Vec::<&crate::families::e5::graph::E5Face>::new())
        })?;
    let mut uses = ctx.collect_indexed_vec(body_faces.len(), "catia_e5_body_uses", |_| {
        Ok(BTreeMap::<u32, usize>::new())
    })?;
    // Every topology edge must be used by the faces of exactly one body.
    let mut edge_bodies = HashMap::<u32, usize>::new();
    for face in ctx.admit_iter(&topology.faces, "catia_e5_body_connectivity_face_scan")? {
        let Some(&body) =
            ctx.get_hash_map(&body_by_face, &face.record_id, "catia_e5_body_faces")?
        else {
            return Ok(None);
        };
        ctx.push_vec(
            &mut body_topology_faces[body],
            face,
            "catia_e5_body_topology_faces",
        )?;
        for loop_ in ctx.admit_iter(&face.loops, "catia_e5_body_connectivity_loop_scan")? {
            for member in
                ctx.admit_iter(&loop_.members, "catia_e5_body_connectivity_member_scan")?
            {
                let edge = member.edge_use;
                if !ctx.contains_key_btree_map(
                    &topology.edges,
                    &edge,
                    "catia_e5_edge_body_members",
                )? {
                    return Ok(None);
                }
                if let Some(previous) =
                    ctx.insert_hash_map(&mut edge_bodies, edge, body, "catia_e5_edge_body_members")?
                {
                    if previous != body {
                        return Ok(None);
                    }
                }
                if let Some(count) =
                    ctx.get_mut_btree_map(&mut uses[body], &edge, "catia_e5_body_edge_uses")?
                {
                    *count += 1;
                } else {
                    ctx.insert_btree_map(&mut uses[body], edge, 1usize, "catia_e5_body_edge_uses")?;
                }
            }
        }
    }
    if body_by_face.len() != topology.faces.len() || edge_bodies.len() != topology.edges.len() {
        return Ok(None);
    }
    let mut plans = Vec::new();
    for ((record_id, faces), (topology_faces, body_uses)) in ctx
        .admit_iter(body_faces, "catia_e5_body_component_source_scan")?
        .zip(body_topology_faces.iter().zip(&uses))
    {
        let mut face_indices = HashMap::new();
        for (index, &face) in ctx
            .admit_iter(faces, "catia_e5_body_component_face_index_scan")?
            .enumerate()
        {
            ctx.insert_hash_map(&mut face_indices, face, index, "catia_e5_face_indices")?;
        }
        let mut parents = UnionFind::charged(ctx, faces.len(), "catia_e5_face_union")?;
        let mut first_face_by_edge = HashMap::<u32, usize>::new();
        for face in ctx.admit_iter(topology_faces, "catia_e5_body_component_topology_face_scan")? {
            let Some(&face_index) =
                ctx.get_hash_map(&face_indices, &face.record_id, "catia_e5_face_indices")?
            else {
                return Ok(None);
            };
            for loop_ in ctx.admit_iter(&face.loops, "catia_e5_body_component_loop_scan")? {
                for member in
                    ctx.admit_iter(&loop_.members, "catia_e5_body_component_member_scan")?
                {
                    let edge = member.edge_use;
                    if let Some(other) = ctx.insert_hash_map(
                        &mut first_face_by_edge,
                        edge,
                        face_index,
                        "catia_e5_first_edge_face",
                    )? {
                        parents.union(ctx, face_index, other)?;
                    }
                }
            }
        }
        // Components are numbered in the order their first face appears.
        let mut labels = ctx.alloc_filled(faces.len(), None, "catia_e5_component_labels")?;
        let mut components = Vec::<Vec<u32>>::new();
        let mut face_components = Vec::new();
        for (face_index, face) in ctx
            .admit_iter(faces, "catia_e5_body_component_face_scan")?
            .copied()
            .enumerate()
        {
            let root = parents.find(ctx, face_index)?;
            let component = match labels[root] {
                Some(component) => component,
                None => {
                    let component = components.len();
                    labels[root] = Some(component);
                    ctx.push_vec(&mut components, Vec::new(), "catia_e5_components")?;
                    component
                }
            };
            ctx.push_vec(&mut face_components, component, "catia_e5_face_components")?;
            ctx.push_vec(&mut components[component], face, "catia_e5_component_faces")?;
        }
        let mut closed_components =
            ctx.alloc_filled(components.len(), true, "catia_e5_closed_components")?;
        let mut component_has_edges =
            ctx.alloc_filled(components.len(), false, "catia_e5_component_edges")?;
        let mut nonmanifold = false;
        for (edge, &count) in ctx.admit_iter(body_uses, "catia_e5_body_edge_use_scan")? {
            let Some(&face) =
                ctx.get_hash_map(&first_face_by_edge, edge, "catia_e5_first_edge_face")?
            else {
                continue;
            };
            let component = face_components[face];
            component_has_edges[component] = true;
            closed_components[component] &= count == 2;
            nonmanifold |= count > 2;
        }
        let closed_component_count = ctx
            .admit_iter(&closed_components, "catia_e5_closed_component_scan")?
            .zip(&component_has_edges)
            .filter(|(closed, has_edges)| **closed && **has_edges)
            .count();
        let kind = if nonmanifold
            || (closed_component_count != 0 && closed_component_count != components.len())
        {
            BodyKind::General
        } else if closed_component_count == components.len() && !components.is_empty() {
            BodyKind::Solid
        } else {
            BodyKind::Sheet
        };
        ctx.push_vec(
            &mut plans,
            E5BodyPlan {
                record_id: *record_id,
                kind,
                components,
            },
            "catia_e5_body_plans",
        )?;
    }
    Ok(Some(plans))
}

/// Collects a surface's faces and fits its plane frame, for tests that
/// name the surface.
#[cfg(test)]
fn plane_frame_for_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface_ref: u32,
    origin: FinitePoint3,
    topology: &crate::families::e5::graph::E5Topology,
    points: &[FinitePoint3],
    expected_normal: Option<Vector3>,
) -> Result<Option<(UnitVector3, UnitVector3, [FiniteReal; 2])>, cadmpeg_core::CodecError> {
    let faces: Vec<_> = topology
        .faces
        .iter()
        .filter(|face| face.surface == surface_ref)
        .collect();
    solve_e5_plane_frame(ctx, origin, &faces, topology, points, expected_normal)
}

/// Plans the boundary with points named by vertex reference, for tests.
#[cfg(test)]
fn plan_boundary_with_points<'a>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    topology: &'a crate::families::e5::graph::E5Topology,
    surface_for_ref: &HashMap<u32, (SurfaceId, &crate::families::e5::records::E5Surface)>,
    points: &HashMap<u32, Point3>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<E5BoundaryPlan<'a>>, cadmpeg_core::CodecError> {
    let model_points: Vec<Point> = topology
        .vertex_refs
        .iter()
        .map(|vertex| {
            Point::new(
                PointId::mint(format!("catia:test:point#{vertex}")).expect("identity grammar"),
                FinitePoint3::new(points[vertex]).expect("finite test point"),
                None,
            )
        })
        .collect();
    let mut scratch = ctx.reserve_scoped(0, "catia_e5_test_boundary_scratch")?;
    plan_e5_boundary(
        ctx,
        &mut scratch,
        topology,
        surface_for_ref,
        E5VertexPoints {
            refs: &topology.vertex_refs,
            points: &model_points,
        },
        refusal,
    )
}

#[cfg(test)]
mod route_tests {
    mod append_admission;
    mod loop_admission;
    mod occurrence_ranges;
    mod ownership_limits;
    mod plane_frames;

    use crate::assemble::quintic_jet_pcurve;
    use crate::families::e5::decode::{
        e5_boundary_curve, e5_circle_carriers_have_same_ordered_sweep, e5_native_uv_endpoints,
        e5_occurrence_intersection_context, e5_ownership_plan, e5_pcurve_on_surface,
        e5_stored_pcurve_reversed, equivalent_e5_curve_carriers, parameter_ranges_reversed,
        plan_boundary_with_points, plane_frame_for_surface, EPS_E5_DECODE_EXACT_GEOMETRY,
        EPS_E5_DECODE_POSITION,
    };
    use crate::families::e5::tests::e5_loop_members;

    use crate::families::e5::graph::{
        E5BoundEntry, E5Bounds, E5CurveSupport, E5CurveSupportKind, E5Edge, E5Face, E5Loop,
        E5OrientedMember, E5Pcurve, E5PcurveJetSite, E5Topology,
    };
    use crate::families::e5::records::E5Surface;

    use cadmpeg_ir::document::CadIr;

    use cadmpeg_ir::geometry::{
        nurbs::NurbsSurface,
        pcurve::{PcurveGeometry, PcurveNurbs},
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{PointId, SurfaceId, VertexId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::{BodyKind, Point, Vertex};
    use cadmpeg_ir::AnnotationBuilder;

    use crate::test_support::test_b5::{finite, finite_lane, finite_pair, point, positive};
    use std::collections::{BTreeMap, HashMap};

    #[test]
    fn e5_unused_surface_stash_refuses_before_collection_growth() {
        let topology = E5Topology {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: BTreeMap::new(),
            pcurves: BTreeMap::new(),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::new(),
            vertex_refs: Vec::new(),
        };
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            let mut ir = CadIr::empty();
            ir.model.surfaces.push(super::Surface {
                id: SurfaceId::compose(
                    &cadmpeg_ir::identity_namespace!("catia", "e5", "surf"),
                    0usize,
                ),
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("valid test plane"),
                )),
                source_object: None,
            });
            let mut unused = Vec::new();
            super::prune_e5_unused_surfaces(
                ctx,
                &mut ir,
                &mut AnnotationBuilder::new(),
                &topology,
                &HashMap::new(),
                (&BTreeMap::new(), &BTreeMap::new()),
                &mut unused,
            )?;
            Ok::<_, cadmpeg_core::CodecError>((ir.model.surfaces, unused))
        };
        let refusal = crate::test_support::with_collection_limit(0, run);
        assert!(
            matches!(refusal, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_e5_unused_surfaces")
        );
        let (surfaces, unused) = crate::test_support::with_service_context(run)
            .expect("service budget admits the unused surface stash");
        assert!(surfaces.is_empty());
        assert_eq!(unused.len(), 1);
    }

    fn rational_pcurve_arc(
        center: [f64; 2],
        radius: f64,
        range: [f64; 2],
        refusal: &mut crate::nurbs::LaneRefusals,
        record: &str,
    ) -> Option<PcurveGeometry> {
        crate::test_support::with_service_context(|ctx| {
            crate::assemble::rational_pcurve_arc(ctx, center, radius, range, refusal, record)
        })
        .expect("service budget admits rational arc")
    }

    #[test]
    fn e5_circle_pcurve_propagates_arc_collection_refusal() {
        let surface = E5Surface {
            pos: 0,
            record_id: 7,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid plane fixture"),
            )),
            uv_scale: finite_pair([1.0, 1.0]),
        };
        let pcurve = E5Pcurve::Circle {
            surface: 0,
            center: finite_pair([0.0, 0.0]),
            codes: [0, 0],
            radius: positive(2.0),
            range: finite_pair([0.0, std::f64::consts::PI]),
            tail: finite_pair([0.0, 0.0]),
        };
        let limited = crate::test_support::with_collection_limit(0, |ctx| {
            e5_pcurve_on_surface(
                ctx,
                &pcurve,
                &surface,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        });
        assert!(
            matches!(limited, Err(cadmpeg_core::CodecError::ResourceLimit(limit))
            if limit.operation == "catia_rational_arc_controls")
        );
        assert!(fixture_pcurve_on_surface(
            &pcurve,
            &surface,
            &mut crate::nurbs::LaneRefusals::new()
        )
        .is_some());
    }

    fn fixture_pcurve_on_surface(
        pcurve: &E5Pcurve,
        surface: &E5Surface,
        refusal: &mut crate::nurbs::LaneRefusals,
    ) -> Option<(PcurveGeometry, [f64; 2], [Point3; 2])> {
        crate::test_support::with_service_context(|ctx| {
            e5_pcurve_on_surface(ctx, pcurve, surface, refusal)
        })
        .expect("service resource budget")
    }

    #[test]
    fn e5_jet_pcurve_refuses_scaled_lane_allocations() {
        let surface = E5Surface {
            pos: 0,
            record_id: 7,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid plane fixture"),
            )),
            uv_scale: finite_pair([1.0, 1.0]),
        };
        let pcurve = jet_pcurve(
            7,
            vec![0.0, 1.0],
            vec![6, 6],
            vec![[0.0, 0.0], [1.0, 0.0]],
            vec![[1.0, 0.0]; 2],
            vec![[0.0, 0.0]; 2],
            [0.0, 1.0],
        );
        let run = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
            e5_pcurve_on_surface(
                ctx,
                &pcurve,
                &surface,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        };
        assert!(crate::test_support::with_service_context(run)
            .expect("service resource budget")
            .is_some());
        assert!(matches!(
            crate::test_support::with_materialized_limit(0, run),
            Err(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.operation == "catia E5 pcurve jet knots"
        ));
        for (cap, operation) in [
            (0, "catia E5 pcurve jet knots"),
            (2, "catia E5 pcurve jet points"),
            (4, "catia E5 pcurve first jets"),
            (6, "catia E5 pcurve second jets"),
        ] {
            assert!(matches!(
                crate::test_support::with_collection_limit(cap, run),
                Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit.operation == operation
            ));
        }
    }

    fn jet_pcurve(
        surface: u32,
        knots: Vec<f64>,
        multiplicities: Vec<u32>,
        points: Vec<[f64; 2]>,
        first_derivatives: Vec<[f64; 2]>,
        second_derivatives: Vec<[f64; 2]>,
        range: [f64; 2],
    ) -> E5Pcurve {
        E5Pcurve::Jet {
            surface,
            sites: E5PcurveJetSite::zip(
                knots.into_iter().map(finite).collect(),
                multiplicities,
                points.into_iter().map(finite_pair).collect(),
                first_derivatives.into_iter().map(finite_pair).collect(),
                second_derivatives.into_iter().map(finite_pair).collect(),
            ),
            range: finite_pair(range),
        }
    }

    #[test]
    fn e5_native_uv_endpoints_reject_nonfinite_results() {
        let line = E5Pcurve::Line {
            surface: 0,
            origin: finite_pair([f64::MAX, 0.0]),
            direction: finite_pair([f64::MAX, 0.0]),
            range: finite_pair([1.0, 2.0]),
        };
        assert!(
            crate::test_support::with_service_context(|ctx| e5_native_uv_endpoints(ctx, &line))
                .expect("service resource budget")
                .is_none()
        );

        let circle = E5Pcurve::Circle {
            surface: 0,
            center: finite_pair([f64::MAX, 0.0]),
            codes: [0, 0],
            radius: positive(f64::MAX),
            range: finite_pair([0.0, 1.0]),
            tail: finite_pair([0.0, 0.0]),
        };
        assert!(
            crate::test_support::with_service_context(|ctx| e5_native_uv_endpoints(ctx, &circle))
                .expect("service resource budget")
                .is_none()
        );
    }

    #[test]
    fn e5_plane_frame_solver_has_no_boundary_segment_cutoff() {
        let segment_count = 17;
        let mut edges = BTreeMap::new();
        let mut pcurves = BTreeMap::new();
        let mut vertex_refs = Vec::with_capacity(2 * segment_count);
        let mut points = Vec::with_capacity(2 * segment_count);
        let mut pcurve_refs = Vec::with_capacity(segment_count);
        let mut edge_refs = Vec::with_capacity(segment_count);

        for index in 0..segment_count {
            let (start_uv, end_uv) = match index {
                0 => ([0.0, 0.0], [1.0, 0.0]),
                1 => ([0.0, 0.0], [0.0, 1.0]),
                _ => {
                    let offset = cadmpeg_core::convert::f64_from_index(index)
                        .expect("fixture index is exactly representable");
                    ([offset, 0.0], [offset + 0.5, 0.0])
                }
            };
            let start_vertex = 1000 + 2 * u32::try_from(index).expect("fixture value fits u32");
            let end_vertex = start_vertex + 1;
            let edge_ref = 3000 + u32::try_from(index).expect("fixture value fits u32");
            let pcurve_ref = 2000 + u32::try_from(index).expect("fixture value fits u32");
            vertex_refs.extend([start_vertex, end_vertex]);
            points.extend([
                point([start_uv[0], start_uv[1], 0.0]),
                point([end_uv[0], end_uv[1], 0.0]),
            ]);
            pcurve_refs.push(pcurve_ref);
            edge_refs.push(edge_ref);
            edges.insert(
                edge_ref,
                E5Edge {
                    support: 0,
                    start_vertex,
                    end_vertex,
                    parameter_start: 0,
                    parameter_end: 0,
                    tail: Vec::new(),
                },
            );
            pcurves.insert(
                pcurve_ref,
                E5Pcurve::Line {
                    surface: 100,
                    origin: finite_pair(start_uv),
                    direction: finite_pair([end_uv[0] - start_uv[0], end_uv[1] - start_uv[1]]),
                    range: finite_pair([0.0, 1.0]),
                },
            );
        }

        let topology = E5Topology {
            bodies: Vec::new(),
            faces: vec![E5Face {
                record_id: 1,
                surface: 100,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![E5Loop {
                    record_id: 2,
                    surface: 100,
                    members: e5_loop_members(&pcurve_refs, &edge_refs, &vec![false; segment_count]),
                    oriented_members: None,
                    outer: Some(true),
                    orientation_hint: None,
                }],
            }],
            edges,
            pcurves,
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::new(),
            vertex_refs,
        };

        let (normal, u_axis, uv_scale) = crate::test_support::with_service_context(|ctx| {
            plane_frame_for_surface(ctx, 100, point([0.0, 0.0, 0.0]), &topology, &points, None)
        })
        .expect("service resource budget")
        .expect("17-segment plane frame");
        assert!(normal.as_raw().dot(Vector3::new(0.0, 0.0, 1.0)) > 1.0 - EPS_E5_DECODE_POSITION);
        assert!(u_axis.as_raw().dot(Vector3::new(1.0, 0.0, 0.0)) > 1.0 - EPS_E5_DECODE_POSITION);
        assert_eq!(uv_scale, finite_pair([1.0, 1.0]));
        assert!(
            crate::test_support::with_service_context(|ctx| plane_frame_for_surface(
                ctx,
                100,
                point([0.0, 0.0, 0.0]),
                &topology,
                &points,
                Some(Vector3::new(f64::NAN, 0.0, 1.0)),
            ))
            .expect("service resource budget")
            .is_none()
        );
    }

    #[test]
    fn e5_plane_frame_solver_reflects_native_chart_for_canonical_u_sign() {
        let topology = E5Topology {
            bodies: Vec::new(),
            faces: vec![E5Face {
                record_id: 1,
                surface: 100,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![E5Loop {
                    record_id: 2,
                    surface: 100,
                    members: e5_loop_members(&[20, 21], &[10, 11], &[false, false]),
                    oriented_members: None,
                    outer: Some(true),
                    orientation_hint: None,
                }],
            }],
            edges: BTreeMap::from([
                (
                    10,
                    E5Edge {
                        support: 0,
                        start_vertex: 1,
                        end_vertex: 2,
                        parameter_start: 0,
                        parameter_end: 0,
                        tail: Vec::new(),
                    },
                ),
                (
                    11,
                    E5Edge {
                        support: 0,
                        start_vertex: 1,
                        end_vertex: 3,
                        parameter_start: 0,
                        parameter_end: 0,
                        tail: Vec::new(),
                    },
                ),
            ]),
            pcurves: BTreeMap::from([
                (
                    20,
                    E5Pcurve::Line {
                        surface: 100,
                        origin: finite_pair([0.0, 0.0]),
                        direction: finite_pair([1.0, 0.0]),
                        range: finite_pair([0.0, 1.0]),
                    },
                ),
                (
                    21,
                    E5Pcurve::Line {
                        surface: 100,
                        origin: finite_pair([0.0, 0.0]),
                        direction: finite_pair([0.0, 1.0]),
                        range: finite_pair([0.0, 1.0]),
                    },
                ),
            ]),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::new(),
            vertex_refs: vec![1, 2, 3],
        };
        let points = vec![
            point([0.0, 0.0, 0.0]),
            point([-1.0, 0.0, 0.0]),
            point([0.0, -1.0, 0.0]),
        ];
        let (normal, u_axis, uv_scale) = crate::test_support::with_service_context(|ctx| {
            plane_frame_for_surface(ctx, 100, point([0.0, 0.0, 0.0]), &topology, &points, None)
        })
        .expect("service resource budget")
        .expect("negative native chart frame");
        assert!(
            normal.as_raw().dot(Vector3::new(0.0, 0.0, 1.0)) > 1.0 - EPS_E5_DECODE_EXACT_GEOMETRY
        );
        assert!(
            u_axis.as_raw().dot(Vector3::new(1.0, 0.0, 0.0)) > 1.0 - EPS_E5_DECODE_EXACT_GEOMETRY
        );
        assert_eq!(uv_scale, finite_pair([-1.0, -1.0]));

        let surface = E5Surface {
            pos: 0,
            record_id: 100,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    *normal.as_raw(),
                    *u_axis.as_raw(),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            uv_scale,
        };
        let (pcurve, range, endpoints) = fixture_pcurve_on_surface(
            &E5Pcurve::Line {
                surface: 100,
                origin: finite_pair([0.0, 0.0]),
                direction: finite_pair([1.0, 0.0]),
                range: finite_pair([0.0, 1.0]),
            },
            &surface,
            &mut crate::nurbs::LaneRefusals::new(),
        )
        .expect("reflected plane pcurve");
        assert_eq!(
            endpoints,
            [Point3::new(0.0, 0.0, 0.0), Point3::new(-1.0, 0.0, 0.0)]
        );
        let PcurveGeometry::Line(line_pcurve) = pcurve else {
            panic!("expected reflected line pcurve");
        };
        let direction = line_pcurve.direction().as_raw();
        assert_eq!(*direction, Point2::new(-1.0, 0.0));
        let (curve, _) = crate::test_support::with_service_context(|ctx| {
            e5_boundary_curve(
                ctx,
                &surface.geometry,
                &E5Pcurve::Line {
                    surface: 100,
                    origin: finite_pair([0.0, 0.0]),
                    direction: finite_pair([1.0, 0.0]),
                    range: finite_pair([0.0, 1.0]),
                },
                &PcurveGeometry::Line(
                    cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                        Point2::new(0.0, 0.0),
                        Point2::new(-1.0, 0.0),
                    )
                    .expect("valid LinePcurve fixture"),
                ),
                (range, endpoints),
                uv_scale,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service profile admits E5 boundary curve")
        .expect("reflected plane boundary");
        assert!(
            matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve))
            if {
                let direction = *line_curve.direction().as_raw();
                direction == Vector3::new(-1.0, 0.0, 0.0)
            })
        );
    }

    #[test]
    fn affine_bound_parameters_preserve_or_reverse_native_direction() {
        assert_eq!(
            parameter_ranges_reversed([0.0, 13.0], [0.0, 122.0]),
            Some(false)
        );
        assert_eq!(
            parameter_ranges_reversed([13.0, 0.0], [0.0, 122.0]),
            Some(true)
        );
        assert_eq!(
            parameter_ranges_reversed([0.0, 1e-200], [0.0, 1e-200]),
            Some(false)
        );
        assert_eq!(parameter_ranges_reversed([1.0, 1.0], [0.0, 1.0]), None);
        assert_eq!(
            parameter_ranges_reversed([0.0, f64::INFINITY], [0.0, 1.0]),
            None
        );
    }

    #[test]
    fn affine_bound_parameters_keep_direction_across_finite_wide_ranges() {
        assert_eq!(
            parameter_ranges_reversed([-f64::MAX, f64::MAX], [-f64::MAX, f64::MAX]),
            Some(false)
        );
        assert_eq!(
            parameter_ranges_reversed([f64::MAX, -f64::MAX], [-f64::MAX, f64::MAX]),
            Some(true)
        );
    }

    #[test]
    fn degenerate_bound_parameters_do_not_select_pcurve_direction() {
        let topology = E5Topology {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: BTreeMap::from([(
                1,
                E5Edge {
                    support: 0,
                    start_vertex: 0,
                    end_vertex: 0,
                    parameter_start: 10,
                    parameter_end: 11,
                    tail: Vec::new(),
                },
            )]),
            pcurves: BTreeMap::new(),
            bounds: BTreeMap::from([
                (
                    10,
                    E5Bounds {
                        entries: vec![E5BoundEntry {
                            representation: 20,
                            parameter: crate::test_support::test_b5::finite(1.0),
                            code: 0,
                        }],
                    },
                ),
                (
                    11,
                    E5Bounds {
                        entries: vec![E5BoundEntry {
                            representation: 20,
                            parameter: crate::test_support::test_b5::finite(1.0),
                            code: 0,
                        }],
                    },
                ),
            ]),
            curve_supports: BTreeMap::new(),
            vertex_refs: Vec::new(),
        };

        assert_eq!(
            crate::test_support::with_service_context(|ctx| e5_stored_pcurve_reversed(
                ctx,
                &topology,
                1,
                20,
                [0.0, 1.0]
            ))
            .expect("service resource budget"),
            None
        );
    }

    #[test]
    fn boundary_planning_rejects_degenerate_occurrence_direction() {
        let surface = E5Surface {
            pos: 0,
            record_id: 100,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            uv_scale: finite_pair([1.0, 1.0]),
        };
        let topology = E5Topology {
            bodies: Vec::new(),
            faces: vec![E5Face {
                record_id: 1,
                surface: 100,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![E5Loop {
                    record_id: 2,
                    surface: 100,
                    members: e5_loop_members(&[20], &[200], &[false]),
                    oriented_members: Some(vec![E5OrientedMember {
                        serialized_index: 0,
                        reversed: false,
                    }]),
                    outer: Some(true),
                    orientation_hint: None,
                }],
            }],
            edges: BTreeMap::from([(
                200,
                E5Edge {
                    support: 300,
                    start_vertex: 400,
                    end_vertex: 401,
                    parameter_start: 500,
                    parameter_end: 501,
                    tail: Vec::new(),
                },
            )]),
            pcurves: BTreeMap::from([(
                20,
                E5Pcurve::Line {
                    surface: 100,
                    origin: finite_pair([0.0, 0.0]),
                    direction: finite_pair([1.0, 0.0]),
                    range: finite_pair([0.0, 1.0]),
                },
            )]),
            bounds: BTreeMap::from([
                (
                    500,
                    E5Bounds {
                        entries: vec![E5BoundEntry {
                            representation: 20,
                            parameter: crate::test_support::test_b5::finite(0.0),
                            code: 0,
                        }],
                    },
                ),
                (
                    501,
                    E5Bounds {
                        entries: vec![E5BoundEntry {
                            representation: 20,
                            parameter: crate::test_support::test_b5::finite(0.0),
                            code: 0,
                        }],
                    },
                ),
            ]),
            curve_supports: BTreeMap::from([(
                300,
                E5CurveSupport {
                    kind: E5CurveSupportKind::Boundary(20),
                    mode: 0,
                    range: finite_pair([0.0, 1.0]),
                    tail: Vec::new(),
                },
            )]),
            vertex_refs: vec![400, 401],
        };
        let surfaces = HashMap::from([(
            100,
            (
                SurfaceId::mint("catia:test:surface#surface".to_string())
                    .expect("identity grammar"),
                &surface,
            ),
        )]);
        let points = HashMap::from([
            (400, Point3::new(0.0, 0.0, 0.0)),
            (401, Point3::new(0.0, 0.0, 0.0)),
        ]);

        assert!(
            crate::test_support::with_service_context(|ctx| plan_boundary_with_points(
                ctx,
                &topology,
                &surfaces,
                &points,
                &mut crate::nurbs::LaneRefusals::new()
            ))
            .expect("service resource budget")
            .is_none()
        );
    }

    #[test]
    fn intersection_side_planning_withholds_ambiguous_degenerate_direction() {
        let surface = E5Surface {
            pos: 0,
            record_id: 100,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            uv_scale: finite_pair([1.0, 1.0]),
        };
        let line = || E5Pcurve::Line {
            surface: 100,
            origin: finite_pair([0.0, 0.0]),
            direction: finite_pair([0.001, 0.0]),
            range: finite_pair([0.0, 1.0]),
        };
        let topology = E5Topology {
            bodies: Vec::new(),
            faces: Vec::new(),
            edges: BTreeMap::from([(
                200,
                E5Edge {
                    support: 300,
                    start_vertex: 400,
                    end_vertex: 401,
                    parameter_start: 500,
                    parameter_end: 501,
                    tail: Vec::new(),
                },
            )]),
            pcurves: BTreeMap::from([(20, line()), (21, line())]),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::from([(
                300,
                E5CurveSupport {
                    kind: E5CurveSupportKind::Intersection([20, 21]),
                    mode: 0,
                    range: finite_pair([0.0, 1.0]),
                    tail: Vec::new(),
                },
            )]),
            vertex_refs: vec![400, 401],
        };
        let surfaces = HashMap::from([(
            100,
            (
                SurfaceId::mint("catia:test:surface#surface".to_string())
                    .expect("identity grammar"),
                &surface,
            ),
        )]);
        let points = HashMap::from([
            (400, Point3::new(0.0001, 0.0, 0.0)),
            (401, Point3::new(0.0004, 0.0, 0.0)),
        ]);

        let plan = crate::test_support::with_service_context(|ctx| {
            plan_boundary_with_points(
                ctx,
                &topology,
                &surfaces,
                &points,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service resource budget")
        .expect("boundary plan");
        assert!(plan.intersections.is_empty());
        assert!(plan.edge_curves.is_empty());
    }

    #[test]
    fn intersection_side_planning_does_not_select_a_conflicting_side() {
        let surface = E5Surface {
            pos: 0,
            record_id: 100,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            uv_scale: finite_pair([1.0, 1.0]),
        };
        let topology = E5Topology {
            bodies: Vec::new(),
            faces: vec![
                E5Face {
                    record_id: 1,
                    surface: 100,
                    trailer_sign: crate::families::e5::graph::Sign::Positive,
                    loops: vec![E5Loop {
                        record_id: 2,
                        surface: 100,
                        members: e5_loop_members(&[20], &[200], &[false]),
                        oriented_members: Some(vec![E5OrientedMember {
                            serialized_index: 0,
                            reversed: false,
                        }]),
                        outer: Some(true),
                        orientation_hint: None,
                    }],
                },
                E5Face {
                    record_id: 3,
                    surface: 100,
                    trailer_sign: crate::families::e5::graph::Sign::Positive,
                    loops: vec![E5Loop {
                        record_id: 4,
                        surface: 100,
                        members: e5_loop_members(&[21], &[200], &[false]),
                        oriented_members: Some(vec![E5OrientedMember {
                            serialized_index: 0,
                            reversed: false,
                        }]),
                        outer: Some(true),
                        orientation_hint: None,
                    }],
                },
            ],
            edges: BTreeMap::from([(
                200,
                E5Edge {
                    support: 300,
                    start_vertex: 400,
                    end_vertex: 401,
                    parameter_start: 500,
                    parameter_end: 501,
                    tail: Vec::new(),
                },
            )]),
            pcurves: BTreeMap::from([
                (
                    20,
                    E5Pcurve::Line {
                        surface: 100,
                        origin: finite_pair([0.0, 0.0]),
                        direction: finite_pair([1.0, 0.0]),
                        range: finite_pair([0.0, 1.0]),
                    },
                ),
                (
                    21,
                    E5Pcurve::Circle {
                        surface: 100,
                        center: finite_pair([0.5, 0.0]),
                        codes: [0, 0],
                        radius: positive(0.5),
                        range: finite_pair([0.0, std::f64::consts::FRAC_PI_2]),
                        tail: finite_pair([0.0, 0.0]),
                    },
                ),
            ]),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::from([(
                300,
                E5CurveSupport {
                    kind: E5CurveSupportKind::Intersection([20, 21]),
                    mode: 0,
                    range: finite_pair([0.0, 1.0]),
                    tail: Vec::new(),
                },
            )]),
            vertex_refs: vec![400, 401],
        };
        let surfaces = HashMap::from([(
            100,
            (
                SurfaceId::mint("catia:test:surface#surface".to_string())
                    .expect("identity grammar"),
                &surface,
            ),
        )]);
        let points = HashMap::from([
            (400, Point3::new(0.0, 0.0, 0.0)),
            (401, Point3::new(1.0, 0.0, 0.0)),
        ]);

        let plan = crate::test_support::with_service_context(|ctx| {
            plan_boundary_with_points(
                ctx,
                &topology,
                &surfaces,
                &points,
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service resource budget")
        .expect("boundary plan");
        assert!(plan.intersections.is_empty());
        assert!(!plan.surface_curves.contains_key(&200));
        assert!(!plan.edge_curves.contains_key(&200));
    }

    #[test]
    fn e5_coedge_pcurve_use_composes_native_and_edge_direction() {
        let topology = E5Topology {
            bodies: Vec::new(),
            faces: vec![E5Face {
                record_id: 1,
                surface: 100,
                trailer_sign: crate::families::e5::graph::Sign::Positive,
                loops: vec![E5Loop {
                    record_id: 2,
                    surface: 100,
                    members: e5_loop_members(&[30], &[20], &[false]),
                    oriented_members: Some(vec![crate::families::e5::graph::E5OrientedMember {
                        serialized_index: 0,
                        reversed: false,
                    }]),
                    outer: Some(true),
                    orientation_hint: None,
                }],
            }],
            edges: BTreeMap::from([(
                20,
                E5Edge {
                    support: 40,
                    start_vertex: 10,
                    end_vertex: 11,
                    parameter_start: 0,
                    parameter_end: 0,
                    tail: Vec::new(),
                },
            )]),
            pcurves: BTreeMap::from([(
                30,
                E5Pcurve::Line {
                    surface: 100,
                    origin: finite_pair([1.0, 0.0]),
                    direction: finite_pair([-1.0, 0.0]),
                    range: finite_pair([0.0, 1.0]),
                },
            )]),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::from([(
                40,
                E5CurveSupport {
                    kind: E5CurveSupportKind::Boundary(30),
                    mode: 0,
                    range: finite_pair([0.0, 1.0]),
                    tail: Vec::new(),
                },
            )]),
            vertex_refs: vec![10, 11],
        };
        let mut ir = CadIr::empty();
        ir.model.points.extend([
            Point::new(
                PointId::mint("catia:test:point#point-10".to_string()).expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            ),
            Point::new(
                PointId::mint("catia:test:point#point-11".to_string()).expect("identity grammar"),
                cadmpeg_ir::features::FinitePoint3::new(Point3::new(1.0, 0.0, 0.0))
                    .expect("a finite position is a point"),
                None,
            ),
        ]);
        ir.model.vertices.extend([
            Vertex {
                id: VertexId::mint("catia:test:vertex#vertex-10".to_string())
                    .expect("identity grammar"),
                point: PointId::mint("catia:test:point#point-10".to_string())
                    .expect("identity grammar"),
                tolerance: None,
            },
            Vertex {
                id: VertexId::mint("catia:test:vertex#vertex-11".to_string())
                    .expect("identity grammar"),
                point: PointId::mint("catia:test:point#point-11".to_string())
                    .expect("identity grammar"),
                tolerance: None,
            },
        ]);
        let surface = crate::families::e5::records::E5Surface {
            pos: 0,
            record_id: 100,
            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .expect("valid PlaneSurface fixture"),
            )),
            uv_scale: finite_pair([1.0, 1.0]),
        };
        let mut annotations = AnnotationBuilder::new();
        crate::test_support::with_service_context(|ctx| {
            let mut admission = super::FamilyEntityAdmission::new(ctx);
            assert!(super::transfer_e5_topology(
                ctx,
                (&mut ir, &mut annotations),
                &topology,
                &[surface],
                &mut crate::nurbs::LaneRefusals::new(),
                &mut admission,
                &mut Vec::new()
            )
            .expect("service limits admit E5 topology"));
        });
        assert_eq!(
            ir.model.coedges[0].pcurves[0]
                .parameter_range
                .map(cadmpeg_ir::geometry::DirectedParameterRange::endpoints),
            Some([1.0, 0.0])
        );
        let [vertex_use] = ir.model.loops[0].anchored_vertex_uses() else {
            panic!("E5 edge emission must retain one vertex use");
        };
        assert_eq!(
            vertex_use.vertex,
            VertexId::mint("catia:e5:v#1".to_string()).expect("identity grammar")
        );
        assert_eq!(vertex_use.after.as_str(), "catia:e5:coedge#2-0");
    }

    #[test]
    fn e5_ownership_requires_complete_bodies_and_partitions_face_components() {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let policy = cadmpeg_core::decode::DecodePolicy::service();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[0], &arena, &policy)
            .expect("fixture fits input limit");
        let face = |record_id, edge_use| E5Face {
            record_id,
            surface: 100 + record_id,
            trailer_sign: crate::families::e5::graph::Sign::Positive,
            loops: vec![E5Loop {
                record_id: 200 + record_id,
                surface: 100 + record_id,
                members: e5_loop_members(&[300 + record_id], &[edge_use], &[false]),
                oriented_members: Some(vec![crate::families::e5::graph::E5OrientedMember {
                    serialized_index: 0,
                    reversed: false,
                }]),
                outer: Some(true),
                orientation_hint: None,
            }],
        };
        let edge = || E5Edge {
            support: 20,
            start_vertex: 30,
            end_vertex: 31,
            parameter_start: 40,
            parameter_end: 41,
            tail: Vec::new(),
        };
        let topology = |faces: Vec<E5Face>, edges: Vec<u32>| E5Topology {
            bodies: Vec::new(),
            faces,
            edges: edges
                .into_iter()
                .map(|record_id| (record_id, edge()))
                .collect(),
            pcurves: BTreeMap::new(),
            bounds: BTreeMap::new(),
            curve_supports: BTreeMap::new(),
            vertex_refs: Vec::new(),
        };

        let plan = e5_ownership_plan(
            &ctx,
            &topology(vec![face(1, 10)], vec![10]),
            &[(None, vec![1])],
        )
        .expect("service resource budget")
        .expect("required invariant");
        assert_eq!(plan[0].kind, BodyKind::Sheet);
        assert_eq!(plan[0].components, vec![vec![1]]);

        let plan = e5_ownership_plan(
            &ctx,
            &topology(vec![face(1, 10), face(2, 10)], vec![10]),
            &[(None, vec![1, 2])],
        )
        .expect("service resource budget")
        .expect("required invariant");
        assert_eq!(plan[0].kind, BodyKind::Solid);
        assert_eq!(plan[0].components, vec![vec![1, 2]]);

        let plan = e5_ownership_plan(
            &ctx,
            &topology(vec![face(1, 10), face(2, 11)], vec![10, 11]),
            &[(None, vec![1, 2])],
        )
        .expect("service resource budget")
        .expect("required invariant");
        assert_eq!(plan[0].kind, BodyKind::Sheet);
        assert_eq!(plan[0].components, vec![vec![1], vec![2]]);

        let plan = e5_ownership_plan(
            &ctx,
            &topology(vec![face(1, 10), face(2, 10), face(3, 11)], vec![10, 11]),
            &[(None, vec![1, 2, 3])],
        )
        .expect("service resource budget")
        .expect("required invariant");
        assert_eq!(plan[0].kind, BodyKind::General);
        assert_eq!(plan[0].components, vec![vec![1, 2], vec![3]]);

        assert!(e5_ownership_plan(
            &ctx,
            &topology(vec![face(1, 10), face(2, 10)], vec![10]),
            &[(Some(1), vec![1]), (Some(2), vec![2])],
        )
        .expect("service resource budget")
        .is_none());
        assert!(e5_ownership_plan(
            &ctx,
            &topology(vec![face(1, 10)], vec![10, 11]),
            &[(None, vec![1])],
        )
        .expect("service resource budget")
        .is_none());
        assert!(
            e5_ownership_plan(&ctx, &topology(Vec::new(), Vec::new()), &[])
                .expect("service resource budget")
                .is_none()
        );
    }

    #[test]
    fn rational_arc_preserves_angular_parameterization() {
        let arc = rational_pcurve_arc(
            [2.0, -3.0],
            4.0,
            [0.0, std::f64::consts::PI],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record",
        )
        .expect("semicircle");
        for (parameter, expected) in [
            (0.0, [6.0, -3.0]),
            (std::f64::consts::FRAC_PI_2, [2.0, 1.0]),
            (std::f64::consts::PI, [-2.0, -3.0]),
        ] {
            let point = cadmpeg_ir::eval::decode::pcurve_uv(
                cadmpeg_ir::eval::admission::EvaluationAdmission::Standard,
                &arc,
                parameter,
            )
            .expect("arc evaluation");
            assert!((point.u - expected[0]).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
            assert!((point.v - expected[1]).abs() < EPS_E5_DECODE_EXACT_GEOMETRY);
        }
    }

    #[test]
    fn rational_arc_rejects_unbounded_subdivision_counts() {
        assert!(rational_pcurve_arc(
            [0.0, 0.0],
            1.0,
            [0.0, 1.0e300],
            &mut crate::nurbs::LaneRefusals::new(),
            "test record"
        )
        .is_none());
    }

    #[test]
    fn e5_cylinder_isoparametric_boundary_lifts_to_circle_carrier() {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("valid CylinderSurface fixture"),
        ));
        let pcurve = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                cadmpeg_ir::math::Point2::new(0.0, 3.0),
                cadmpeg_ir::math::Point2::new(1.0, 0.0),
            )
            .expect("valid LinePcurve fixture"),
        );
        let native = crate::families::e5::graph::E5Pcurve::Line {
            surface: 0,
            origin: finite_pair([0.0, 3.0]),
            direction: finite_pair([1.0, 0.0]),
            range: finite_pair([0.0, std::f64::consts::FRAC_PI_2]),
        };
        let (curve, range) = crate::test_support::with_service_context(|ctx| {
            e5_boundary_curve(
                ctx,
                &surface,
                &native,
                &pcurve,
                (
                    [0.0, std::f64::consts::FRAC_PI_2],
                    [Point3::new(2.0, 0.0, 3.0), Point3::new(0.0, 2.0, 3.0)],
                ),
                finite_pair([1.0, 1.0]),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service profile admits E5 boundary curve")
        .expect("cylinder boundary circle");
        assert!(
            matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve))
                    if {
                        let center = circle_curve.center().get();
            let radius = circle_curve.radius().get();
                        center == Point3::new(0.0, 0.0, 3.0) && radius == 2.0
                    })
        );
        assert!(
            (range[1] - range[0] - std::f64::consts::FRAC_PI_2).abs()
                < EPS_E5_DECODE_EXACT_GEOMETRY
        );
    }

    #[test]
    fn e5_nearly_isoparametric_boundary_lifts_to_circle_carrier() {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("valid CylinderSurface fixture"),
        ));
        let transverse_noise = f64::EPSILON;
        let pcurve = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 3.0),
                Point2::new(1.0, transverse_noise),
            )
            .expect("valid LinePcurve fixture"),
        );
        let native = crate::families::e5::graph::E5Pcurve::Line {
            surface: 0,
            origin: finite_pair([0.0, 3.0]),
            direction: finite_pair([1.0, transverse_noise]),
            range: finite_pair([0.0, std::f64::consts::FRAC_PI_2]),
        };
        let (curve, _) = crate::test_support::with_service_context(|ctx| {
            e5_boundary_curve(
                ctx,
                &surface,
                &native,
                &pcurve,
                (
                    [0.0, std::f64::consts::FRAC_PI_2],
                    [Point3::new(2.0, 0.0, 3.0), Point3::new(0.0, 2.0, 3.0)],
                ),
                finite_pair([1.0, 1.0]),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service profile admits E5 boundary curve")
        .expect("near-isoparametric cylinder boundary circle");
        assert!(
            matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) if { circle_curve.radius().get() == 2.0 })
        );
    }

    #[test]
    fn e5_boundary_classification_is_scale_independent() {
        let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(
            cadmpeg_ir::geometry::analytic::CylinderSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                2.0,
            )
            .expect("valid CylinderSurface fixture"),
        ));
        let direction = 1e-200;
        let parameter_end = 1e200;
        assert!(matches!(
            super::e5_isoparametric_direction(Point2::new(direction, 0.0)),
            Some(super::E5IsoparametricDirection::ConstantV)
        ));
        let pcurve = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 3.0),
                Point2::new(1.0, 0.0),
            )
            .expect("valid LinePcurve fixture"),
        );
        let native = crate::families::e5::graph::E5Pcurve::Line {
            surface: 0,
            origin: finite_pair([0.0, 3.0]),
            direction: finite_pair([direction, 0.0]),
            range: finite_pair([0.0, parameter_end]),
        };
        let (curve, _) = crate::test_support::with_service_context(|ctx| {
            e5_boundary_curve(
                ctx,
                &surface,
                &native,
                &pcurve,
                (
                    [0.0, direction * parameter_end],
                    [
                        Point3::new(2.0, 0.0, 3.0),
                        Point3::new(2.0 * 1.0f64.cos(), 2.0 * 1.0f64.sin(), 3.0),
                    ],
                ),
                finite_pair([1.0, 1.0]),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service profile admits E5 boundary curve")
        .expect("cylinder boundary circle");
        assert!(
            matches!(curve, CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) if { circle_curve.radius().get() == 2.0 })
        );

        let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .expect("valid PlaneSurface fixture"),
        ));
        let plane_pcurve = PcurveGeometry::Line(
            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(1.0, 0.0),
            )
            .expect("valid LinePcurve fixture"),
        );
        let plane_native = crate::families::e5::graph::E5Pcurve::Line {
            surface: 0,
            origin: finite_pair([0.0, 0.0]),
            direction: finite_pair([direction, 0.0]),
            range: finite_pair([0.0, 1.0]),
        };
        let tiny_endpoint = Point3::new(direction, 0.0, 0.0);
        let (curve, range) = crate::test_support::with_service_context(|ctx| {
            e5_boundary_curve(
                ctx,
                &plane,
                &plane_native,
                &plane_pcurve,
                (
                    [0.0, direction],
                    [Point3::new(0.0, 0.0, 0.0), tiny_endpoint],
                ),
                finite_pair([1.0, 1.0]),
                &mut crate::nurbs::LaneRefusals::new(),
            )
        })
        .expect("service profile admits E5 boundary curve")
        .expect("finite nonzero plane line");
        assert!(matches!(
            curve,
            CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
        ));
        assert_eq!(range, [0.0, direction]);
    }

    mod boundary_cases;
}
