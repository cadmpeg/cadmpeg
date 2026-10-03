// SPDX-License-Identifier: Apache-2.0
//! Transfer of reference-closed `b5 03` object topology into neutral IR.
//!
//! [`transfer`] drives a two-phase lowering: [`build_plan`] resolves the whole
//! graph into a [`TransferPlan`] of cross-pass id tables, then per-IR-layer emit
//! passes ([`vertices`], [`surfaces`], [`pcurves`], [`edges`], [`faces`]) append
//! neutral records in a fixed order. Each pass owns exactly one model layer and
//! reads only the plan fields its layer needs.

use cadmpeg_core::decode::u64_from_index;

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use crate::families::FamilyEntityAdmission;
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{
    nurbs::NurbsCurve,
    pcurve::{PcurveGeometry, PcurveNurbs},
    CurveGeometry, ProceduralCurveDefinition, ProceduralSurfaceDefinition, RecordBounds,
    SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::scalar::{FiniteReal, PositiveReal};
use cadmpeg_ir::topology::{BodyKind, IncreasingParameterInterval};
use cadmpeg_ir::units::UnitVector3;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::graph::{
    bounded_occurrence_range, edge_pcurve_parameters, face_loop_owner_counts, loop_chain_closes,
    pcurve_nurbs_knots, pcurve_parameter_domain, B5ExtrusionDirectrix, B5ExtrusionSurface, B5Graph,
    B5OffsetSurface, B5SupportedSurface, B5Surface,
};

mod edges;
mod faces;
mod pcurves;
pub(in crate::families) mod surfaces;
mod vertices;

use edges::{
    b5_supports_agree, b5_supports_follow_curve, b5_supports_follow_edge, merge_curve_plan,
    orient_b5_supports_to_edge,
};
use faces::{orient_loop_members, ownership_plan};
use pcurves::{
    cylinder_helix, isocurve_endpoint_parameters, lifted_curve_geometry, neutral_pcurve_point,
    nurbs_isocurve, oriented_circle_plan, oriented_line_plan, oriented_nurbs_range,
    sphere_great_circle_geometry, sphere_great_circle_pcurve,
};
use vertices::transfer_vertex_tolerances;

/// CATIA's object-stream on-carrier incidence tolerance, in millimetres.
const POINT_TOLERANCE: f64 = 1e-3;

/// Acceptance radius for matching one lifted support endpoint to its edge
/// endpoint.
///
/// `vertex_tolerances` holds no source-stated tolerance. Every entry is an
/// evaluation residual this decoder computed from lifted pcurve geometry, and a
/// vertex with no entry produced no residual at all. The radius is therefore
/// the larger of that residual and the object-stream incidence tolerance: the
/// gate never runs tighter than the format's own incidence tolerance, and a
/// stated tolerance is never floored, because none reaches here.
fn endpoint_gate_radius(residual: Option<PositiveReal>) -> f64 {
    match residual.map(PositiveReal::get) {
        Some(residual) if residual > POINT_TOLERANCE => residual,
        _ => POINT_TOLERANCE,
    }
}

type B5Support = (u32, u32, [FiniteReal; 2]);
type B5SupportPlan = HashMap<u32, Vec<B5Support>>;

struct RevolutionPlan {
    directrix: NurbsCurve,
    axis_origin: FinitePoint3,
    axis_direction: UnitVector3,
    angular_interval: [f64; 2],
    angular_parameter_interval: [f64; 2],
    parameter_interval: [f64; 2],
}

enum SurfaceProcedure {
    Extrusion(Box<ResolvedExtrusionSurface>),
    Revolution(RevolutionPlan),
    RollingBall {
        carrier_object_id: u32,
        definition: Box<ProceduralSurfaceDefinition>,
    },
}

struct SurfacePlan {
    geometry: SurfaceGeometry,
    procedure: Option<SurfaceProcedure>,
}

#[derive(Debug, Clone, PartialEq)]
struct CurvePlan {
    geometry: CurveGeometry,
    parameter_range: Option<[f64; 2]>,
    edge_tolerance: Option<cadmpeg_ir::scalar::PositiveReal>,
    cache_fit_tolerance: Option<cadmpeg_ir::geometry::FitTolerance>,
}

#[derive(Debug, Clone, PartialEq)]
struct HelixPlan {
    definition: ProceduralCurveDefinition,
    cache: NurbsCurve,
    parameter_range: [f64; 2],
    fit_tolerance: cadmpeg_ir::geometry::FitTolerance,
}

struct OwnershipPlan {
    body_kind: BodyKind,
    face_components: Vec<usize>,
    loop_owners: HashMap<u32, usize>,
}

impl OwnershipPlan {
    fn components(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<BTreeMap<usize, Vec<usize>>, cadmpeg_core::CodecError> {
        let mut components = BTreeMap::<usize, Vec<usize>>::new();
        for (face, &component) in self.face_components.iter().enumerate() {
            ctx.admit_btree_entry(&components, &component, "catia_b5_face_component_groups")?;
            ctx.push_vec(
                components.entry(component).or_default(),
                face,
                "catia_b5_face_component_members",
            )?;
        }
        Ok(components)
    }
}

struct OrientedLoop {
    flipped: bool,
    members: Vec<OrientedLoopMember>,
}

#[derive(Clone, Copy)]
struct OrientedLoopMember {
    reversed: bool,
    pcurve_reversed: bool,
}

impl OrientedLoop {
    fn member_order(&self) -> impl DoubleEndedIterator<Item = usize> + '_ {
        let n = self.members.len();
        (0..n).map(move |i| if self.flipped { n - 1 - i } else { i })
    }
}

/// Cross-pass id tables and resolved geometry plans shared between the emit
/// passes. Each field is produced by [`build_plan`] and consumed by exactly the
/// passes named in its doc comment; `edge_curve_plan`, `edge_helix_plan`,
/// `edge_ids`, and `surface_plan` are drained by their consuming pass.
struct TransferPlan {
    /// Face ownership components and body kind (read by `faces`).
    ownership: OwnershipPlan,
    /// Neutral surface plans keyed by object id (drained by `surfaces`).
    surface_plan: BTreeMap<u32, SurfacePlan>,
    /// Pcurve geometry, cylinder-reparameterization flag, and native range
    /// keyed by object id (read by `pcurves` and `edges`).
    pcurve_plan: BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
    /// Oriented 3D curve plans keyed by edge id (drained by `edges`).
    edge_curve_plan: HashMap<u32, CurvePlan>,
    /// Cylinder helix procedural plans keyed by edge id (drained by `edges`).
    edge_helix_plan: HashMap<u32, HelixPlan>,
    /// Ordered support occurrences per edge (read by `edges`).
    edge_support_plan: B5SupportPlan,
    /// Every edge id used by a transferred loop member (drained by `edges`).
    edge_ids: BTreeSet<u32>,
    /// Solved member order and coedge senses per loop (read by `faces`).
    loop_orientation: BTreeMap<u32, OrientedLoop>,
    /// Endpoint tolerances keyed by vertex index (read by `vertices`).
    vertex_tolerances: BTreeMap<usize, cadmpeg_ir::scalar::PositiveReal>,
    /// Edges whose supports reproduce the edge endpoints (read by `edges`).
    exact_support_edges: HashSet<u32>,
    /// Edges whose supports reproduce the lifted curve (read by `edges`).
    exact_support_curves: HashSet<u32>,
    /// Vertex indices referenced by a transferred edge (read by `vertices`).
    used_vertices: HashSet<usize>,
}

/// Transfer a complete B5 graph. Returns `false` without mutation when any
/// referenced face, pcurve, edge endpoint, or loop chain remains unresolved.
pub(in crate::families) fn transfer(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    mut graph: B5Graph,
    payload: &UnknownId,
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    if !graph.complete {
        graph.loops.retain(|_, loop_| {
            loop_.members.iter().all(|member| {
                (graph
                    .pcurves
                    .get(&member.pcurve)
                    .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                    || graph
                        .opaque_pcurves
                        .get(&member.pcurve)
                        .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                    || graph.implicit_pcurves.get(&member.pcurve) == Some(&loop_.surface))
                    && graph.vertices.edges().contains_key(&member.edge)
            }) && loop_chain_closes(loop_, graph.vertices.edges())
        });
        graph.faces.retain(|face| {
            graph.surfaces.contains_key(&face.surface)
                && !face.loops.is_empty()
                && face.loops.iter().all(|loop_id| {
                    graph
                        .loops
                        .get(loop_id)
                        .is_some_and(|loop_| loop_.surface == face.surface)
                })
        });
        let loop_owner_counts = face_loop_owner_counts(admission.ctx, &graph.faces)?;
        graph.faces.retain(|face| {
            face.loops
                .iter()
                .all(|loop_id| loop_owner_counts.get(loop_id).copied() == Some(1))
        });
        let mut referenced_loops = HashSet::new();
        for face in &graph.faces {
            for &loop_id in &face.loops {
                admission.ctx.insert_hash_set(
                    &mut referenced_loops,
                    loop_id,
                    "catia_b5_transfer_referenced_loops",
                )?;
            }
        }
        graph
            .loops
            .retain(|loop_id, _| referenced_loops.contains(loop_id));
        if graph.faces.is_empty() || graph.loops.is_empty() {
            return Ok(false);
        }
        graph.complete = true;
    }
    transfer_complete(ir, annotations, &graph, payload, refusal, admission)
}

/// Orchestrate the staged emit passes over a resolved [`TransferPlan`]. The pass
/// order fixes the neutral-model arena and annotation order and must not change.
fn transfer_complete(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    graph: &B5Graph,
    payload: &UnknownId,
    refusal: &mut crate::nurbs::LaneRefusals,
    admission: &mut FamilyEntityAdmission<'_, '_>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let Some(mut plan) = build_plan(admission.ctx, graph, payload, refusal)? else {
        return Ok(false);
    };
    if let Err(error) = vertices::emit_vertices(ir, annotations, graph, &plan, admission) {
        return semantic_fallthrough_or_resource(error);
    }
    let surface_ids = match surfaces::emit_surfaces(ir, annotations, graph, &mut plan, admission) {
        Ok(ids) => ids,
        Err(error) => return semantic_fallthrough_or_resource(error),
    };
    let pcurve_uses = match pcurves::emit_pcurves(ir, annotations, graph, &plan, admission) {
        Ok(uses) => uses,
        Err(error) => return semantic_fallthrough_or_resource(error),
    };
    let edge_id_map = match edges::emit_edges(
        ir,
        annotations,
        graph,
        payload,
        &mut plan,
        &surface_ids,
        admission,
    ) {
        Ok(ids) => ids,
        Err(error) => return semantic_fallthrough_or_resource(error),
    };
    if !faces::emit_faces(
        ir,
        annotations,
        graph,
        &plan,
        &faces::EmittedFaceInputs {
            surface_ids: &surface_ids,
            pcurve_uses: &pcurve_uses,
            edge_ids: &edge_id_map,
        },
        admission,
    )? {
        return Ok(false);
    }
    Ok(true)
}

fn semantic_fallthrough_or_resource(
    error: cadmpeg_core::CodecError,
) -> Result<bool, cadmpeg_core::CodecError> {
    match error {
        cadmpeg_core::CodecError::ResourceLimit(_) => Err(error),
        _ => Ok(false),
    }
}

fn add_referenced_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    referenced: &mut HashSet<u32>,
    pending: &mut Vec<u32>,
    surface_id: u32,
) -> Result<(), cadmpeg_core::CodecError> {
    if ctx.insert_hash_set(referenced, surface_id, "catia_b5_referenced_surface_ids")? {
        ctx.push_vec(pending, surface_id, "catia_b5_pending_surface_ids")?;
    }
    Ok(())
}

fn referenced_surface_ids(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    roots: impl IntoIterator<Item = u32>,
    offsets: &BTreeMap<u32, B5OffsetSurface>,
    supported: &BTreeMap<u32, B5SupportedSurface>,
    extrusions: &BTreeMap<u32, B5ExtrusionSurface>,
    aliases: &BTreeMap<u32, u32>,
) -> Result<HashSet<u32>, cadmpeg_core::CodecError> {
    let mut referenced = HashSet::new();
    let mut pending = Vec::new();
    for root in roots {
        add_referenced_surface(ctx, &mut referenced, &mut pending, root)?;
    }
    while let Some(surface_id) = pending.pop() {
        let Some(construction_id) = super::graph::canonical_surface_id(aliases, surface_id) else {
            continue;
        };
        if let Some(offset) = offsets.get(&construction_id) {
            add_referenced_surface(ctx, &mut referenced, &mut pending, offset.source_surface)?;
            add_referenced_surface(ctx, &mut referenced, &mut pending, offset.carrier_surface)?;
        } else if let Some(construction) = supported.get(&construction_id) {
            for &support in &construction.support_surfaces {
                add_referenced_surface(ctx, &mut referenced, &mut pending, support)?;
            }
            add_referenced_surface(
                ctx,
                &mut referenced,
                &mut pending,
                construction.carrier_surface,
            )?;
        } else if let Some(extrusion) = extrusions.get(&construction_id) {
            for (support, _, _) in extrusion.directrix.supports() {
                add_referenced_surface(ctx, &mut referenced, &mut pending, *support)?;
            }
        }
    }
    Ok(referenced)
}

/// Resolve the whole graph into the cross-pass [`TransferPlan`]. Returns `None`
/// when any referenced surface, pcurve, edge endpoint, or loop chain fails to
/// close so the caller leaves the model untouched.
fn build_plan(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    payload: &UnknownId,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<TransferPlan>, cadmpeg_core::CodecError> {
    (|| -> Option<Result<TransferPlan, cadmpeg_core::CodecError>> {
        macro_rules! admitted {
            ($result:expr) => {
                match $result {
                    Ok(value) => value,
                    Err(error) => return Some(Err(error)),
                }
            };
        }
        if graph.faces.is_empty() {
            return None;
        }

        let ownership = match ownership_plan(ctx, graph) {
            Ok(Some(ownership)) => ownership,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };

        let referenced_surfaces = match referenced_surface_ids(
            ctx,
            graph.faces.iter().map(|face| face.surface),
            &graph.offset_surfaces,
            &graph.supported_surfaces,
            &graph.extrusion_surfaces,
            &graph.surface_aliases,
        ) {
            Ok(referenced) => referenced,
            Err(error) => return Some(Err(error)),
        };
        let mut surface_plan = BTreeMap::new();
        for surface_id in referenced_surfaces {
            let surface = graph.surfaces.get(&surface_id)?;
            let plan = match surfaces::neutral_surface(
                ctx, surface, graph, surface_id, payload, refusal,
            ) {
                Ok(plan) => plan,
                Err(error) => return Some(Err(error)),
            };
            admitted!(ctx.insert_btree_map(
                &mut surface_plan,
                surface_id,
                plan,
                "catia_b5_transfer_surface_plan"
            ));
        }

        let mut pcurve_plan = BTreeMap::new();
        let mut edge_curve_plan = HashMap::<u32, CurvePlan>::new();
        let mut conflicting_edge_curves = HashSet::<u32>::new();
        let mut edge_helix_plan = HashMap::<u32, HelixPlan>::new();
        let mut edge_support_plan = B5SupportPlan::new();
        let mut loop_senses = BTreeMap::new();
        let mut edge_ids = BTreeSet::new();
        for loop_ in graph.loops.values() {
            if loop_.members.is_empty() {
                return None;
            }
            let owner = ownership.loop_owners.get(&loop_.object_id).copied()?;
            if graph.faces.get(owner)?.surface != loop_.surface {
                return None;
            }
            if !loop_chain_closes(loop_, graph.vertices.edges()) {
                return None;
            }
            let senses = match loop_.edge_senses(ctx) {
                Ok(senses) => senses,
                Err(error) => return Some(Err(error)),
            };
            if let Err(error) = ctx.insert_btree_map(
                &mut loop_senses,
                loop_.object_id,
                senses,
                "catia_b5_transfer_loop_senses",
            ) {
                return Some(Err(error));
            }
            for member in &loop_.members {
                let pcurve_id = member.pcurve;
                let edge_id = member.edge;
                let Some(pcurve) = graph.pcurves.get(&pcurve_id) else {
                    if let Some(opaque) = graph
                        .opaque_pcurves
                        .get(&pcurve_id)
                        .filter(|pcurve| pcurve.surface == loop_.surface)
                    {
                        if let Some((pcurve_geometry, parameter_range, geometry)) = opaque
                            .sphere_great_circle
                            .as_ref()
                            .and_then(|pcurve| {
                                let (pcurve_geometry, parameter_range) =
                                    sphere_great_circle_pcurve(pcurve)?;
                                let geometry = sphere_great_circle_geometry(
                                    pcurve,
                                    graph.surfaces.get(&loop_.surface)?,
                                )?;
                                Some((pcurve_geometry, parameter_range, geometry))
                            })
                            .filter(|(_, _, geometry)| {
                                let Some(points) = graph.vertices.edge_points(edge_id) else {
                                    return false;
                                };
                                circle_contains_points(geometry, &points)
                            })
                        {
                            admitted!(ctx.admit_btree_entry(
                                &pcurve_plan,
                                &pcurve_id,
                                "catia_b5_transfer_pcurve_plan"
                            ));
                            pcurve_plan.entry(pcurve_id).or_insert((
                                pcurve_geometry,
                                false,
                                parameter_range,
                            ));
                            let support_range = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                                .and_then(|parameters| {
                                    bounded_occurrence_range(parameters, parameter_range)
                                })
                                .unwrap_or(parameter_range);
                            admitted!(ctx.admit_hash_map_entry(
                                &mut edge_support_plan,
                                &edge_id,
                                "catia_b5_edge_support_groups"
                            ));
                            let supports = edge_support_plan.entry(edge_id).or_default();
                            if !supports.iter().any(|(surface, pcurve, range)| {
                                *surface == loop_.surface
                                    && *pcurve == pcurve_id
                                    && *range == support_range
                            }) {
                                admitted!(ctx.push_vec(
                                    supports,
                                    (loop_.surface, pcurve_id, support_range),
                                    "catia_b5_edge_supports"
                                ));
                            }
                            if let Err(error) = merge_curve_plan(
                                ctx,
                                &mut edge_curve_plan,
                                &mut conflicting_edge_curves,
                                edge_id,
                                CurvePlan {
                                    geometry,
                                    parameter_range: None,
                                    edge_tolerance: None,
                                    cache_fit_tolerance: None,
                                },
                            ) {
                                return Some(Err(error));
                            }
                        }
                        admitted!(ctx.insert_btree_set(
                            &mut edge_ids,
                            edge_id,
                            "catia_b5_transfer_edge_ids"
                        ));
                        continue;
                    }
                    if graph.implicit_pcurves.get(&pcurve_id) == Some(&loop_.surface) {
                        admitted!(ctx.insert_btree_set(
                            &mut edge_ids,
                            edge_id,
                            "catia_b5_transfer_edge_ids"
                        ));
                        continue;
                    }
                    return None;
                };
                if pcurve.surface != loop_.surface || !graph.vertices.edges().contains_key(&edge_id)
                {
                    return None;
                }
                let knots = match pcurve_nurbs_knots(ctx, pcurve) {
                    Ok(Some(knots)) => knots,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let knots = match ctx.collect_vec(
                    knots.into_iter().map(FiniteReal::get),
                    "catia_b5_transfer_pcurve_knots",
                ) {
                    Ok(knots) => knots,
                    Err(error) => return Some(Err(error)),
                };
                let parameter_range = pcurve_parameter_domain(pcurve)?;
                let surface = graph.surfaces.get(&loop_.surface)?;
                let cylinder_reparameterized = matches!(surface, B5Surface::Cylinder { .. });
                let points = admitted!(ctx.collect_vec(
                    pcurve
                        .control_points
                        .iter()
                        .map(|point| neutral_pcurve_point(point.get(), surface)),
                    "catia_b5_transfer_pcurve_points"
                ));
                let weights = match pcurve.weights.as_ref() {
                    Some(weights) => Some(admitted!(ctx.collect_vec(
                        weights.iter().copied().map(PositiveReal::get),
                        "catia_b5_transfer_pcurve_weights"
                    ))),
                    None => None,
                };
                let geometry = PcurveGeometry::Nurbs {
                    nurbs: admitted!(crate::nurbs::note_refusal(
                        ctx,
                        admitted!(PcurveNurbs::from_lanes(
                            ctx,
                            pcurve.degree,
                            knots,
                            points,
                            weights,
                            false,
                        )),
                        refusal,
                        format_args!("b5 object-stream pcurve record #{}", pcurve.object_id),
                    ))?,
                };
                admitted!(ctx.admit_btree_entry(
                    &pcurve_plan,
                    &pcurve_id,
                    "catia_b5_transfer_pcurve_plan"
                ));
                pcurve_plan.entry(pcurve_id).or_insert((
                    geometry,
                    cylinder_reparameterized,
                    parameter_range,
                ));
                admitted!(ctx.admit_hash_map_entry(
                    &mut edge_support_plan,
                    &edge_id,
                    "catia_b5_edge_support_groups"
                ));
                let supports = edge_support_plan.entry(edge_id).or_default();
                let support_range = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                    .and_then(|parameters| bounded_occurrence_range(parameters, parameter_range))
                    .unwrap_or(parameter_range);
                if !supports.iter().any(|(surface, pcurve, range)| {
                    *surface == loop_.surface && *pcurve == pcurve_id && *range == support_range
                }) {
                    admitted!(ctx.push_vec(
                        supports,
                        (loop_.surface, pcurve_id, support_range),
                        "catia_b5_edge_supports"
                    ));
                }
                let lifted = match lifted_curve_geometry(ctx, pcurve, surface) {
                    Ok(lifted) => lifted,
                    Err(error) => return Some(Err(error)),
                };
                let lifted = if lifted.is_some() {
                    lifted
                } else {
                    match surface_plan.get(&loop_.surface) {
                        Some(SurfacePlan {
                            geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(cache)),
                            ..
                        }) => match nurbs_isocurve(ctx, pcurve, cache) {
                            Ok(curve) => curve
                                .map(SolvedCurveGeometry::Nurbs)
                                .map(CurveGeometry::Solved),
                            Err(error) => return Some(Err(error)),
                        },
                        _ => None,
                    }
                };
                if let Some(geometry) = lifted {
                    let [edge_start, edge_end] = graph.vertices.edge_points(edge_id)?;
                    let oriented_plan = if matches!(surface, B5Surface::Plane { .. }) {
                        match edge_pcurve_parameters(graph, edge_id, pcurve_id) {
                            Some(parameters) => admitted!(oriented_nurbs_range(
                                ctx,
                                &geometry,
                                parameters.map(FiniteReal::get),
                                edge_start,
                                edge_end
                            )),
                            None => None,
                        }
                    } else if matches!(surface, B5Surface::Nurbs(_) | B5Surface::Revolution { .. })
                    {
                        let parameters = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                            .map(|parameters| parameters.map(FiniteReal::get));
                        let parameters = match parameters {
                            Some(parameters) => {
                                match isocurve_endpoint_parameters(ctx, pcurve, parameters) {
                                    Ok(parameters) => parameters,
                                    Err(error) => return Some(Err(error)),
                                }
                            }
                            None => None,
                        };
                        match parameters {
                            Some(parameters) => admitted!(oriented_nurbs_range(
                                ctx, &geometry, parameters, edge_start, edge_end
                            )),
                            None => None,
                        }
                    } else if matches!(
                        geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Line(_))
                    ) {
                        oriented_line_plan(&geometry, edge_start, edge_end)
                    } else if matches!(
                        geometry,
                        CurveGeometry::Solved(SolvedCurveGeometry::Circle(_))
                    ) {
                        let parameters = edge_pcurve_parameters(graph, edge_id, pcurve_id);
                        match parameters {
                            Some(parameters) => match oriented_circle_plan(
                                ctx,
                                pcurve,
                                surface,
                                &geometry,
                                parameters.map(FiniteReal::get),
                                edge_start,
                                edge_end,
                            ) {
                                Ok(plan) => plan,
                                Err(error) => return Some(Err(error)),
                            },
                            None => None,
                        }
                    } else {
                        None
                    };
                    let plan = oriented_plan.unwrap_or(CurvePlan {
                        geometry,
                        parameter_range: None,
                        edge_tolerance: None,
                        cache_fit_tolerance: None,
                    });
                    if let Err(error) = merge_curve_plan(
                        ctx,
                        &mut edge_curve_plan,
                        &mut conflicting_edge_curves,
                        edge_id,
                        plan,
                    ) {
                        return Some(Err(error));
                    }
                    if conflicting_edge_curves.contains(&edge_id) {
                        edge_helix_plan.remove(&edge_id);
                    }
                } else {
                    let [edge_start, edge_end] = graph.vertices.edge_points(edge_id)?;
                    let Some(endpoint_parameters) =
                        edge_pcurve_parameters(graph, edge_id, pcurve_id)
                    else {
                        admitted!(ctx.insert_btree_set(
                            &mut edge_ids,
                            edge_id,
                            "catia_b5_transfer_edge_ids"
                        ));
                        continue;
                    };
                    let helix = match cylinder_helix(
                        ctx,
                        pcurve,
                        surface,
                        endpoint_parameters.map(FiniteReal::get),
                        edge_start,
                        edge_end,
                        refusal,
                    ) {
                        Ok(helix) => helix,
                        Err(error) => return Some(Err(error)),
                    };
                    let Some(helix) = helix else {
                        admitted!(ctx.insert_btree_set(
                            &mut edge_ids,
                            edge_id,
                            "catia_b5_transfer_edge_ids"
                        ));
                        continue;
                    };
                    if edge_helix_plan
                        .get(&edge_id)
                        .is_some_and(|existing| existing != &helix)
                    {
                        return None;
                    }
                    if let Err(error) = merge_curve_plan(
                        ctx,
                        &mut edge_curve_plan,
                        &mut conflicting_edge_curves,
                        edge_id,
                        CurvePlan {
                            geometry: CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(admitted!(
                                helix
                                    .cache
                                    .try_clone_for_decode(ctx, "catia_b5_helix_plan_curve")
                            ))),
                            parameter_range: Some(helix.parameter_range),
                            edge_tolerance: Some(cadmpeg_ir::scalar::PositiveReal::new(
                                helix.fit_tolerance.get(),
                            )?),
                            cache_fit_tolerance: Some(helix.fit_tolerance),
                        },
                    ) {
                        return Some(Err(error));
                    }
                    if conflicting_edge_curves.contains(&edge_id) {
                        edge_helix_plan.remove(&edge_id);
                    } else {
                        admitted!(ctx.admit_hash_map_entry(
                            &mut edge_helix_plan,
                            &edge_id,
                            "catia_b5_edge_helix_plans"
                        ));
                        edge_helix_plan.entry(edge_id).or_insert(helix);
                    }
                }
                admitted!(ctx.insert_btree_set(
                    &mut edge_ids,
                    edge_id,
                    "catia_b5_transfer_edge_ids"
                ));
            }
        }
        let loop_orientation = match orient_loop_members(ctx, graph, loop_senses) {
            Ok(Some(orientation)) => orientation,
            Ok(None) => return None,
            Err(error) => return Some(Err(error)),
        };
        let vertex_tolerances = admitted!(transfer_vertex_tolerances(
            ctx,
            graph,
            &edge_support_plan,
            &surface_plan,
            &pcurve_plan
        ));
        for (&edge, supports) in &mut edge_support_plan {
            let vertices = graph.vertices.edges()[&edge];
            let [start, end] = graph.vertices.edge_points(edge)?;
            let tolerances = vertices.map(|vertex| {
                endpoint_gate_radius(
                    vertex_tolerances
                        .get(&vertex.combined_index(graph.vertices.raw_points().len()))
                        .copied(),
                )
            });
            if let Err(error) = orient_b5_supports_to_edge(
                ctx,
                supports,
                [start, end],
                tolerances,
                &surface_plan,
                &pcurve_plan,
            ) {
                return Some(Err(error));
            }
        }
        let mut exact_support_edges = HashSet::new();
        for (&edge, supports) in &edge_support_plan {
            let Some(&vertices) = graph.vertices.edges().get(&edge) else {
                continue;
            };
            let Some([start, end]) = graph.vertices.edge_points(edge) else {
                continue;
            };
            let tolerances = vertices.map(|vertex| {
                endpoint_gate_radius(
                    vertex_tolerances
                        .get(&vertex.combined_index(graph.vertices.raw_points().len()))
                        .copied(),
                )
            });
            let follows = match b5_supports_follow_edge(
                ctx,
                supports,
                [start, end],
                tolerances,
                &surface_plan,
                &pcurve_plan,
            ) {
                Ok(follows) => follows,
                Err(error) => return Some(Err(error)),
            };
            if follows {
                admitted!(ctx.insert_hash_set(
                    &mut exact_support_edges,
                    edge,
                    "catia_b5_exact_support_edges"
                ));
            }
        }
        let mut exact_support_curves = HashSet::new();
        for (&edge, supports) in &edge_support_plan {
            let follows = match edge_curve_plan.get(&edge).map_or_else(
                || b5_supports_agree(ctx, supports, &surface_plan, &pcurve_plan),
                |plan| b5_supports_follow_curve(ctx, supports, plan, &surface_plan, &pcurve_plan),
            ) {
                Ok(follows) => follows,
                Err(error) => return Some(Err(error)),
            };
            if follows {
                admitted!(ctx.insert_hash_set(
                    &mut exact_support_curves,
                    edge,
                    "catia_b5_exact_support_curves"
                ));
            }
        }

        let mut used_vertices = HashSet::new();
        for edge in &edge_ids {
            for vertex in graph.vertices.edges()[edge] {
                admitted!(ctx.insert_hash_set(
                    &mut used_vertices,
                    vertex.combined_index(graph.vertices.raw_points().len()),
                    "catia_b5_used_vertices"
                ));
            }
        }

        Some(Ok(TransferPlan {
            ownership,
            surface_plan,
            pcurve_plan,
            edge_curve_plan,
            edge_helix_plan,
            edge_support_plan,
            edge_ids,
            loop_orientation,
            vertex_tolerances,
            exact_support_edges,
            exact_support_curves,
            used_vertices,
        }))
    })()
    .transpose()
}

pub(in crate::families) fn resolved_surface_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    surface_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<SurfaceGeometry>, cadmpeg_core::CodecError> {
    let Some(surface) = graph.surfaces.get(&surface_id) else {
        return Ok(None);
    };
    let payload = UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "payload", "unknown"),
        cadmpeg_ir::identity_key!("b5-surface"),
    );
    let geometry =
        surfaces::neutral_surface(ctx, surface, graph, surface_id, &payload, refusal)?.geometry;
    Ok((!matches!(
        geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Unknown { .. })
    ))
    .then_some(geometry))
}

/// Exact construction of a surface-of-revolution carrier.
#[derive(Clone, PartialEq)]
pub(in crate::families) struct ResolvedRevolutionSurface {
    /// Exact profile curve used as the revolution directrix.
    pub(in crate::families) directrix: NurbsCurve,
    /// Point on the revolution axis.
    pub(in crate::families) axis_origin: FinitePoint3,
    /// Unit revolution-axis direction.
    pub(in crate::families) axis_direction: UnitVector3,
    /// Angular interval in radians.
    pub(in crate::families) angular_interval: [f64; 2],
    /// Native angular surface-parameter interval mapped to `angular_interval`.
    pub(in crate::families) angular_parameter_interval: [f64; 2],
    /// Native profile parameter interval.
    pub(in crate::families) parameter_interval: [f64; 2],
}

/// Resolve a surface-of-revolution construction with an exact NURBS result.
pub(in crate::families) fn resolved_revolution_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    surface_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<ResolvedRevolutionSurface>, cadmpeg_core::CodecError> {
    let Some(surface) = graph.surfaces.get(&surface_id) else {
        return Ok(None);
    };
    let payload = UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "payload", "unknown"),
        cadmpeg_ir::identity_key!("b5-surface"),
    );
    let SurfacePlan {
        geometry,
        procedure,
    } = surfaces::neutral_surface(ctx, surface, graph, surface_id, &payload, refusal)?;
    let Some(SurfaceProcedure::Revolution(plan)) = procedure else {
        return Ok(None);
    };
    if !matches!(
        geometry,
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
    ) {
        return Ok(None);
    }
    Ok(Some(ResolvedRevolutionSurface {
        directrix: plan.directrix,
        axis_origin: plan.axis_origin,
        axis_direction: plan.axis_direction,
        angular_interval: plan.angular_interval,
        angular_parameter_interval: plan.angular_parameter_interval,
        parameter_interval: plan.parameter_interval,
    }))
}

#[derive(Clone, PartialEq)]
/// One object-stream pcurve lowered with its exact resolved support carrier.
pub(in crate::families) struct ResolvedObjectStreamPcurve {
    /// Persistent identity of the pcurve's support surface.
    pub(in crate::families) surface_object_id: u32,
    /// Exact neutral support construction.
    pub(in crate::families) carrier: ResolvedPcurveSurface,
    /// Exact neutral parameter-space curve.
    pub(in crate::families) geometry: PcurveGeometry,
    /// Native pcurve parameter interval.
    pub(in crate::families) parameter_range: [f64; 2],
}

/// Exact neutral carrier for an identity-bound object-stream pcurve.
#[derive(Clone, PartialEq)]
pub(in crate::families) enum ResolvedPcurveSurface {
    /// Direct neutral surface geometry.
    Geometry(SurfaceGeometry),
    /// Procedural rolling-ball carrier.
    RollingBall {
        /// Persistent result-carrier identity.
        carrier_object_id: u32,
        /// Exact rolling-ball definition.
        definition: Box<ProceduralSurfaceDefinition>,
    },
}

/// Lower one resolved object-stream surface to an exact neutral carrier.
pub(in crate::families) fn resolved_surface_carrier(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    surface: &B5Surface,
) -> Result<Option<ResolvedPcurveSurface>, cadmpeg_core::CodecError> {
    Ok(match surfaces::surface_carrier(ctx, surface)? {
        surfaces::B5SurfaceCarrier::Analytic(geometry) => {
            Some(ResolvedPcurveSurface::Geometry(geometry))
        }
        surfaces::B5SurfaceCarrier::Procedural(surfaces::B5ProceduralSurface::RollingBall {
            carrier_object_id,
            definition,
        }) => Some(ResolvedPcurveSurface::RollingBall {
            carrier_object_id,
            definition: {
                let copy = surfaces::copy_rolling_ball_definition(ctx, definition)?;
                ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<ProceduralSurfaceDefinition>()),
                    "catia_b5_resolved_rolling_ball_box",
                )?;
                Box::new(copy)
            },
        }),
        surfaces::B5SurfaceCarrier::Procedural(
            surfaces::B5ProceduralSurface::Unresolved
            | surfaces::B5ProceduralSurface::Revolution { .. },
        ) => None,
    })
}

/// Resolve a pcurve support carrier with the graph context required by exact
/// constructed surfaces such as surface-of-revolution records.
pub(in crate::families) fn resolved_surface_carrier_in_graph(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    surface_object_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<ResolvedPcurveSurface>, cadmpeg_core::CodecError> {
    let Some(surface) = graph.surfaces.get(&surface_object_id) else {
        return Ok(None);
    };
    if let Some(carrier) = resolved_surface_carrier(ctx, surface)? {
        return Ok(Some(carrier));
    }
    Ok(
        resolved_surface_geometry(ctx, graph, surface_object_id, refusal)?
            .map(ResolvedPcurveSurface::Geometry),
    )
}

/// Lower one decoded degree-5 UV jet through its resolved native chart.
pub(in crate::families) fn resolved_object_stream_pcurve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    pcurve: &crate::families::a5a8::records::A8Pcurve,
    surface: &B5Surface,
    graph: Option<&B5Graph>,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<ResolvedObjectStreamPcurve>, cadmpeg_core::CodecError> {
    let graph_carrier = match graph {
        Some(graph) => resolved_surface_carrier_in_graph(ctx, graph, pcurve.support_id, refusal)?,
        None => None,
    };
    let Some(carrier) = (match graph_carrier {
        Some(carrier) => Some(carrier),
        None => resolved_surface_carrier(ctx, surface)?,
    }) else {
        return Ok(None);
    };
    let Some((knots, control_points)) = pcurve.bspline(ctx)? else {
        return Ok(None);
    };
    let Some(nurbs) = crate::nurbs::note_refusal(
        ctx,
        PcurveNurbs::from_lanes(
            ctx,
            crate::families::a5a8::records::A8Pcurve::DEGREE,
            knots,
            ctx.collect_vec(
                control_points
                    .into_iter()
                    .map(|point| pcurves::neutral_pcurve_point(point.get(), surface)),
                "catia_b5_object_stream_pcurve_points",
            )?,
            None,
            false,
        )?,
        refusal,
        format_args!("a8 object-stream pcurve record #{}", pcurve.support_id),
    )?
    else {
        return Ok(None);
    };
    Ok(Some(ResolvedObjectStreamPcurve {
        surface_object_id: pcurve.support_id,
        carrier,
        geometry: PcurveGeometry::Nurbs { nurbs },
        parameter_range: pcurve.range.map(FiniteReal::get),
    }))
}

pub(in crate::families) fn resolved_surface_procedural_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    surface_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<(u32, ProceduralSurfaceDefinition)>, cadmpeg_core::CodecError> {
    let Some(surface) = graph.surfaces.get(&surface_id) else {
        return Ok(None);
    };
    let payload = UnknownId::compose(
        &cadmpeg_ir::identity_namespace!("catia", "payload", "unknown"),
        cadmpeg_ir::identity_key!("b5-surface"),
    );
    let procedure = surfaces::neutral_surface(ctx, surface, graph, surface_id, &payload, refusal)?;
    Ok(match procedure.procedure {
        Some(SurfaceProcedure::RollingBall {
            carrier_object_id,
            definition,
        }) => Some((carrier_object_id, *definition)),
        Some(SurfaceProcedure::Extrusion(_) | SurfaceProcedure::Revolution(_)) | None => None,
    })
}

/// Neutral support evidence for one side of an exact extrusion directrix.
#[derive(Clone, PartialEq)]
pub(in crate::families) struct ResolvedExtrusionSupport {
    /// Persistent support-surface identity.
    pub(in crate::families) surface_object_id: u32,
    /// Exact neutral support geometry.
    pub(in crate::families) surface: SurfaceGeometry,
    /// Exact parameter-space directrix occurrence.
    pub(in crate::families) pcurve: PcurveGeometry,
    /// Native interval used by this support occurrence.
    pub(in crate::families) pcurve_parameter_range: [f64; 2],
    /// Exact model-space lift when the support chart admits one.
    curve: Option<CurveGeometry>,
}

/// Exact neutral construction of one extrusion directrix.
#[derive(Clone, PartialEq)]
pub(in crate::families) enum ResolvedExtrusionDirectrix {
    /// Intersection of two support surfaces.
    Intersection {
        /// Ordered exact support sides.
        supports: Box<[ResolvedExtrusionSupport; 2]>,
        /// Positive fit tolerance of the retained sampled cache.
        cache_fit_tolerance: PositiveReal,
    },
    /// One pcurve lifted through its exact support surface.
    SurfaceCurve {
        /// Exact support side.
        support: ResolvedExtrusionSupport,
        /// Exact model-space curve lifted through the support.
        curve: CurveGeometry,
    },
    /// Fixed-direction offset of a one-support source curve.
    Offset {
        /// Persistent source-curve wrapper identity.
        source_object_id: u32,
        /// Exact source support side.
        support: ResolvedExtrusionSupport,
        /// Exact model-space source curve lifted through the support.
        source_curve: CurveGeometry,
        /// Increasing source-curve interval.
        source_parameter_range: IncreasingParameterInterval,
        /// Signed offset distance.
        distance: FiniteReal,
        /// Unit direction defining the positive offset side.
        direction: UnitVector3,
    },
}

/// Exact two-support directrix and extrusion chart resolved from B5 objects.
#[derive(Clone, PartialEq)]
pub(in crate::families) struct ResolvedExtrusionSurface {
    /// Persistent extrusion-surface identity.
    pub(in crate::families) surface_object_id: u32,
    /// Persistent directrix identity.
    pub(in crate::families) directrix_object_id: u32,
    /// Solved directrix interval shared by the support mappings.
    pub(in crate::families) directrix_parameter_range: IncreasingParameterInterval,
    /// Unit world-space extrusion direction.
    pub(in crate::families) direction: UnitVector3,
    /// Increasing native U and V chart bounds.
    pub(in crate::families) parameter_bounds: [IncreasingParameterInterval; 2],
    /// Exact directrix construction.
    pub(in crate::families) directrix: ResolvedExtrusionDirectrix,
}

impl ResolvedExtrusionSurface {
    pub(in crate::families) fn supports(&self) -> impl Iterator<Item = &ResolvedExtrusionSupport> {
        let (first, second) = match &self.directrix {
            ResolvedExtrusionDirectrix::Intersection { supports, .. } => {
                (Some(&supports[0]), Some(&supports[1]))
            }
            ResolvedExtrusionDirectrix::SurfaceCurve { support, .. }
            | ResolvedExtrusionDirectrix::Offset { support, .. } => (Some(support), None),
        };
        first.into_iter().chain(second)
    }
}

fn copy_resolved_extrusion_support(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    support: &ResolvedExtrusionSupport,
) -> Result<ResolvedExtrusionSupport, cadmpeg_core::CodecError> {
    let surface = support
        .surface
        .try_clone_for_decode(ctx, "catia_b5_extrusion_support_surface_copy")?;
    Ok(ResolvedExtrusionSupport {
        surface_object_id: support.surface_object_id,
        surface,
        pcurve: support
            .pcurve
            .try_clone_for_decode(ctx, "catia_b5_extrusion_support_pcurve_copy")?,
        pcurve_parameter_range: support.pcurve_parameter_range,
        curve: support
            .curve
            .as_ref()
            .map(|curve| copy_lifted_curve(ctx, curve))
            .transpose()?,
    })
}

pub(in crate::families) fn copy_resolved_extrusion_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    value: &ResolvedExtrusionSurface,
) -> Result<ResolvedExtrusionSurface, cadmpeg_core::CodecError> {
    let directrix = match &value.directrix {
        ResolvedExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
        } => {
            ctx.charge_retained(
                u64_from_index(std::mem::size_of::<[ResolvedExtrusionSupport; 2]>()),
                "catia_b5_extrusion_support_pair_copy",
            )?;
            ResolvedExtrusionDirectrix::Intersection {
                supports: Box::new([
                    copy_resolved_extrusion_support(ctx, &supports[0])?,
                    copy_resolved_extrusion_support(ctx, &supports[1])?,
                ]),
                cache_fit_tolerance: *cache_fit_tolerance,
            }
        }
        ResolvedExtrusionDirectrix::SurfaceCurve { support, curve } => {
            ResolvedExtrusionDirectrix::SurfaceCurve {
                support: copy_resolved_extrusion_support(ctx, support)?,
                curve: copy_lifted_curve(ctx, curve)?,
            }
        }
        ResolvedExtrusionDirectrix::Offset {
            source_object_id,
            support,
            source_curve,
            source_parameter_range,
            distance,
            direction,
        } => ResolvedExtrusionDirectrix::Offset {
            source_object_id: *source_object_id,
            support: copy_resolved_extrusion_support(ctx, support)?,
            source_curve: copy_lifted_curve(ctx, source_curve)?,
            source_parameter_range: *source_parameter_range,
            distance: *distance,
            direction: *direction,
        },
    };
    Ok(ResolvedExtrusionSurface {
        surface_object_id: value.surface_object_id,
        directrix_object_id: value.directrix_object_id,
        directrix_parameter_range: value.directrix_parameter_range,
        direction: value.direction,
        parameter_bounds: value.parameter_bounds,
        directrix,
    })
}

/// Exact support construction of a resolved offset surface.
#[derive(Clone, PartialEq)]
pub(in crate::families) enum ResolvedOffsetSupport {
    /// Direct neutral support geometry.
    Geometry(SurfaceGeometry),
    /// Procedural extrusion support.
    Extrusion(Box<ResolvedExtrusionSurface>),
}

/// Exact offset construction resolved from a B5 class-`30` object.
#[derive(Clone, PartialEq)]
pub(in crate::families) struct ResolvedOffsetSurface {
    /// Persistent result-carrier identity.
    pub(in crate::families) carrier_object_id: u32,
    /// Persistent support-surface identity.
    pub(in crate::families) support_object_id: u32,
    /// Exact support construction.
    pub(in crate::families) support: ResolvedOffsetSupport,
    /// Signed offset distance.
    pub(in crate::families) distance: FiniteReal,
    /// Increasing native U and V chart bounds.
    pub(in crate::families) parameter_bounds: [IncreasingParameterInterval; 2],
}

/// Record bounds of a native U and V chart pair, in the native field order
/// `[u_lower, u_upper, v_lower, v_upper]`.
pub(in crate::families) fn parameter_record_bounds(
    [u_bounds, v_bounds]: [IncreasingParameterInterval; 2],
) -> RecordBounds {
    let [u0, u1] = u_bounds.finite_endpoints();
    let [v0, v1] = v_bounds.finite_endpoints();
    RecordBounds::from_finite([u0, u1, v0, v1])
}

pub(in crate::families) fn resolved_extrusion_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    surface_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<ResolvedExtrusionSurface>, cadmpeg_core::CodecError> {
    (|| -> Option<Result<ResolvedExtrusionSurface, cadmpeg_core::CodecError>> {
        let construction_id = graph.canonical_surface_id(surface_id)?;
        let extrusion = graph.extrusion_surfaces.get(&construction_id)?;
        let active = extrusion.parameter_bounds[1];
        let mut resolve_support = |(
            surface_object_id,
            pcurve_object_id,
            pcurve_parameter_range,
        ): (u32, u32, [FiniteReal; 2])|
         -> Result<
            Option<ResolvedExtrusionSupport>,
            cadmpeg_core::CodecError,
        > {
            (|| -> Option<Result<ResolvedExtrusionSupport, cadmpeg_core::CodecError>> {
                let source_surface = graph.surfaces.get(&surface_object_id)?;
                let surface =
                    match resolved_surface_geometry(ctx, graph, surface_object_id, refusal) {
                        Ok(Some(surface)) => surface,
                        Ok(None) => return None,
                        Err(error) => return Some(Err(error)),
                    };
                let pcurve = graph.pcurves.get(&pcurve_object_id)?;
                let knots = match pcurve_nurbs_knots(ctx, pcurve) {
                    Ok(Some(knots)) => knots,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let knots = match ctx.collect_vec(
                    knots.into_iter().map(FiniteReal::get),
                    "catia_b5_extrusion_pcurve_knots",
                ) {
                    Ok(knots) => knots,
                    Err(error) => return Some(Err(error)),
                };
                let domain = pcurve_parameter_domain(pcurve)?;
                bounded_occurrence_range(pcurve_parameter_range, domain)?;
                let points = match ctx.collect_vec(
                    pcurve
                        .control_points
                        .iter()
                        .map(|point| neutral_pcurve_point(point.get(), source_surface)),
                    "catia_b5_extrusion_pcurve_points",
                ) {
                    Ok(points) => points,
                    Err(error) => return Some(Err(error)),
                };
                let weights = match pcurve
                    .weights
                    .as_ref()
                    .map(|weights| {
                        ctx.collect_vec(
                            weights.iter().copied().map(PositiveReal::get),
                            "catia_b5_extrusion_pcurve_weights",
                        )
                    })
                    .transpose()
                {
                    Ok(weights) => weights,
                    Err(error) => return Some(Err(error)),
                };
                let nurbs = match crate::nurbs::note_refusal(
                    ctx,
                    match PcurveNurbs::from_lanes(ctx, pcurve.degree, knots, points, weights, false)
                    {
                        Ok(result) => result,
                        Err(error) => return Some(Err(error)),
                    },
                    refusal,
                    format_args!("b5 extrusion pcurve record #{pcurve_object_id}"),
                ) {
                    Ok(Some(nurbs)) => nurbs,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let pcurve_geometry = PcurveGeometry::Nurbs { nurbs };
                let curve = match lifted_curve_geometry(ctx, pcurve, source_surface) {
                    Ok(curve) => curve,
                    Err(error) => return Some(Err(error)),
                };
                Some(Ok(ResolvedExtrusionSupport {
                    surface_object_id,
                    surface,
                    pcurve: pcurve_geometry,
                    pcurve_parameter_range: pcurve_parameter_range.map(FiniteReal::get),
                    curve,
                }))
            })()
            .transpose()
        };
        let directrix = match &extrusion.directrix {
            B5ExtrusionDirectrix::Intersection {
                supports,
                cache_fit_tolerance,
                ..
            } => {
                let [left, right] = supports.map(&mut resolve_support);
                let left = match left {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let right = match right {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let supports = [left, right];
                (supports[0].surface_object_id != supports[1].surface_object_id).then_some(())?;
                if let Err(error) = ctx.charge_retained(
                    u64_from_index(std::mem::size_of::<[ResolvedExtrusionSupport; 2]>()),
                    "catia_b5_extrusion_intersection_support_box",
                ) {
                    return Some(Err(error));
                }
                ResolvedExtrusionDirectrix::Intersection {
                    supports: Box::new(supports),
                    cache_fit_tolerance: *cache_fit_tolerance,
                }
            }
            B5ExtrusionDirectrix::SurfaceCurve { support, .. } => {
                let support = match resolve_support(*support) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let curve = match copy_lifted_curve(ctx, support.curve.as_ref()?) {
                    Ok(curve) => curve,
                    Err(error) => return Some(Err(error)),
                };
                let curve = match curve_on_parameter_range(
                    ctx,
                    curve,
                    support.pcurve_parameter_range,
                    active,
                    &format_args!(
                        "b5 extrusion directrix on surface record #{}",
                        support.surface_object_id
                    ),
                    refusal,
                ) {
                    Ok(Some(curve)) => curve,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                ResolvedExtrusionDirectrix::SurfaceCurve { support, curve }
            }
            B5ExtrusionDirectrix::Offset {
                source,
                source_parameter_range,
                distance,
                direction,
                ..
            } => {
                let B5ExtrusionDirectrix::SurfaceCurve {
                    object_id, support, ..
                } = source.as_ref()
                else {
                    return None;
                };
                let support = match resolve_support(*support) {
                    Ok(Some(value)) => value,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                let source_curve = match copy_lifted_curve(ctx, support.curve.as_ref()?) {
                    Ok(curve) => curve,
                    Err(error) => return Some(Err(error)),
                };
                let source_curve = match curve_on_parameter_range(
                    ctx,
                    source_curve,
                    source_parameter_range.endpoints(),
                    active,
                    &format_args!(
                        "b5 offset extrusion source curve on surface record #{}",
                        support.surface_object_id
                    ),
                    refusal,
                ) {
                    Ok(Some(curve)) => curve,
                    Ok(None) => return None,
                    Err(error) => return Some(Err(error)),
                };
                ResolvedExtrusionDirectrix::Offset {
                    source_object_id: *object_id,
                    support,
                    source_curve,
                    source_parameter_range: active,
                    distance: *distance,
                    direction: *direction,
                }
            }
        };
        Some(Ok(ResolvedExtrusionSurface {
            surface_object_id: surface_id,
            directrix_object_id: extrusion.directrix.object_id(),
            directrix_parameter_range: active,
            direction: extrusion.direction,
            parameter_bounds: extrusion.parameter_bounds,
            directrix,
        }))
    })()
    .transpose()
}

fn copy_lifted_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: &CurveGeometry,
) -> Result<CurveGeometry, cadmpeg_core::CodecError> {
    Ok(match curve {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) => {
            CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                nurbs.try_clone_for_decode(ctx, "catia_b5_extrusion_lifted_curve_copy")?,
            ))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line)) => {
            CurveGeometry::Solved(SolvedCurveGeometry::Line(*line))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle)) => {
            CurveGeometry::Solved(SolvedCurveGeometry::Circle(*circle))
        }
        _ => {
            return Err(cadmpeg_core::CodecError::malformed(
                "B5 lifted extrusion curve has unsupported geometry",
            ))
        }
    })
}

fn curve_on_parameter_range(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    curve: CurveGeometry,
    source: [f64; 2],
    target: IncreasingParameterInterval,
    record: &dyn std::fmt::Display,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<CurveGeometry>, cadmpeg_core::CodecError> {
    let Some(source_interval) = IncreasingParameterInterval::new(source) else {
        return Ok(None);
    };
    let target_interval = target;
    let target = target_interval.endpoints();
    if target
        .into_iter()
        .all(|value| cadmpeg_ir::math::parameter_in_domain(value, source, 64.0 * f64::EPSILON))
    {
        return Ok(Some(curve));
    }
    let source_span = source[1] - source[0];
    let target_span = target[1] - target[0];
    let target_per_source = target_span / source_span;
    let source_per_target = source_span / target_span;
    match curve {
        CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(mut curve)) => {
            let mapped = ctx.collect_vec(
                curve
                    .knots()
                    .iter()
                    .map(|knot| target[0] + (*knot - source[0]) * target_per_source),
                "catia_b5_reparameterized_curve_knots",
            )?;
            let mapped = if mapped.iter().all(|knot| knot.is_finite()) {
                mapped
            } else {
                let mapped = ctx.collect_options(
                    curve.knots().iter().map(|knot| {
                        target_interval
                            .map_from(
                                source_interval,
                                cadmpeg_ir::scalar::FiniteReal::new(*knot)?,
                                false,
                            )
                            .ok()
                            .map(cadmpeg_ir::scalar::FiniteReal::get)
                    }),
                    "catia_b5_reparameterized_curve_fallback_knots",
                )?;
                let Some(mapped) = mapped else {
                    return Ok(None);
                };
                mapped
            };
            if curve
                .edit_knots(ctx, |knots| knots.copy_from_slice(&mapped))?
                .is_err()
            {
                return Ok(None);
            }
            Ok(Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
                curve,
            ))))
        }
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            if source_per_target != 1.0 {
                return crate::nurbs::note_refusal(
                    ctx,
                    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                        ctx,
                        1,
                        ctx.collect_vec(
                            [target[0], target[0], target[1], target[1]],
                            "catia_b5_reparameterized_line_knots",
                        )?,
                        ctx.collect_vec(
                            source.into_iter().map(|parameter| {
                                Point3::new(
                                    origin.x + parameter * direction.x,
                                    origin.y + parameter * direction.y,
                                    origin.z + parameter * direction.z,
                                )
                            }),
                            "catia_b5_reparameterized_line_points",
                        )?,
                        None,
                        false,
                    )?,
                    refusal,
                    format_args!(
                        "b5 line curve reparameterized onto its occurrence range: {record}"
                    ),
                )
                .map(|curve| {
                    curve
                        .map(SolvedCurveGeometry::Nurbs)
                        .map(CurveGeometry::Solved)
                });
            }
            Ok(Some(CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::new(
                    match FinitePoint3::new(Point3::new(
                        origin.x + (source[0] - target[0] * source_per_target) * direction.x,
                        origin.y + (source[0] - target[0] * source_per_target) * direction.y,
                        origin.z + (source[0] - target[0] * source_per_target) * direction.z,
                    )) {
                        Some(point) => point,
                        None => return Ok(None),
                    },
                    line_curve.direction(),
                ),
            ))))
        }
        _ => Ok(None),
    }
}

pub(in crate::families) fn resolved_offset_surface(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    graph: &B5Graph,
    surface_id: u32,
    refusal: &mut crate::nurbs::LaneRefusals,
) -> Result<Option<ResolvedOffsetSurface>, cadmpeg_core::CodecError> {
    let Some(construction_id) = graph.canonical_surface_id(surface_id) else {
        return Ok(None);
    };
    let Some(offset) = graph.offset_surfaces.get(&construction_id) else {
        return Ok(None);
    };
    let support = if let Some(geometry) =
        resolved_surface_geometry(ctx, graph, offset.source_surface, refusal)?
    {
        ResolvedOffsetSupport::Geometry(geometry)
    } else if let Some(extrusion) =
        resolved_extrusion_surface(ctx, graph, offset.source_surface, refusal)?
    {
        ResolvedOffsetSupport::Extrusion(Box::new(extrusion))
    } else {
        return Ok(None);
    };
    Ok(Some(ResolvedOffsetSurface {
        carrier_object_id: offset.carrier_surface,
        support_object_id: offset.source_surface,
        support,
        distance: offset.distance,
        parameter_bounds: offset.parameter_bounds,
    }))
}

fn annotate(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    id: impl std::fmt::Display,
    stream: &str,
    tag: &str,
    exactness: Exactness,
) -> Result<(), cadmpeg_core::CodecError> {
    annotations.annotate(ctx, id, format_args!("catia:{stream}"), 0, tag, exactness)
}

fn vector(value: [f64; 3]) -> Vector3 {
    Vector3::new(value[0], value[1], value[2])
}

fn point3(value: [f64; 3]) -> Point3 {
    Point3::new(value[0], value[1], value[2])
}

fn subtract(left: [f64; 3], right: [f64; 3]) -> [f64; 3] {
    (Vector3::from(left) - Vector3::from(right)).into()
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    Vector3::from(left).dot(Vector3::from(right))
}

fn length(value: [f64; 3]) -> f64 {
    Vector3::from(value).norm()
}

fn circle_contains_points(geometry: &CurveGeometry, points: &[[f64; 3]]) -> bool {
    let CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) = geometry else {
        return false;
    };
    let center = circle_curve.center().get();
    let axis = circle_curve.frame().axis().as_raw();
    let radius = circle_curve.radius().get();
    let center = [center.x, center.y, center.z];
    let axis = [axis.x, axis.y, axis.z];
    points.iter().all(|point| {
        let offset = subtract(*point, center);
        (length(offset) - radius).abs() <= POINT_TOLERANCE
            && dot(offset, axis).abs() <= POINT_TOLERANCE
    })
}

#[cfg(test)]
mod tests;
