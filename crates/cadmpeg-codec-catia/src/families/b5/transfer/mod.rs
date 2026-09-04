// SPDX-License-Identifier: Apache-2.0
//! Transfer of reference-closed `b5 03` object topology into neutral IR.
//!
//! [`transfer`] drives a two-phase lowering: [`build_plan`] resolves the whole
//! graph into a [`TransferPlan`] of cross-pass id tables, then per-IR-layer emit
//! passes ([`vertices`], [`surfaces`], [`pcurves`], [`edges`], [`faces`]) append
//! neutral records in a fixed order. Each pass owns exactly one model layer and
//! reads only the plan fields its layer needs.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::geometry::{
    CurveGeometry, NurbsCurve, PcurveGeometry, PcurveNurbs, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::graph::{
    bounded_occurrence_range, edge_pcurve_parameters, face_loop_owner_counts, loop_chain_closes,
    pcurve_nurbs_knots, pcurve_parameter_domain, B5ExtrusionDirectrix, B5ExtrusionSurface, B5Graph,
    B5OffsetSurface, B5SupportedSurface, B5Surface,
};

mod edges;
mod faces;
mod pcurves;
mod surfaces;
mod vertices;

use edges::{
    b5_supports_agree, b5_supports_follow_curve, b5_supports_follow_edge, b5_vertex_point,
    curve_cache_has_ordered_knots, merge_curve_plan, orient_b5_supports_to_edge,
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

type B5Support = (u32, u32, [f64; 2]);
type B5SupportPlan = HashMap<u32, Vec<B5Support>>;

struct RevolutionPlan {
    directrix: NurbsCurve,
    axis_origin: Point3,
    axis_direction: Vector3,
    angular_interval: [f64; 2],
    angular_parameter_interval: [f64; 2],
    parameter_interval: [f64; 2],
}

#[allow(clippy::large_enum_variant)]
enum SurfaceProcedure {
    Extrusion(Box<ResolvedExtrusionSurface>),
    Revolution(RevolutionPlan),
    RollingBall {
        carrier_object_id: u32,
        definition: ProceduralSurfaceDefinition,
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
    edge_tolerance: Option<f64>,
    cache_fit_tolerance: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
struct HelixPlan {
    definition: ProceduralCurveDefinition,
    cache: NurbsCurve,
    parameter_range: [f64; 2],
    fit_tolerance: f64,
}

struct OwnershipPlan {
    body_kind: BodyKind,
    components: Vec<Vec<usize>>,
    face_components: Vec<usize>,
    loop_owners: HashMap<u32, usize>,
}

struct OrientedLoop {
    member_order: Vec<usize>,
    reversed: Vec<bool>,
    pcurve_reversed: Vec<bool>,
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
    pcurve_plan: BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
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
    vertex_tolerances: BTreeMap<usize, f64>,
    /// Edges whose supports reproduce the edge endpoints (read by `edges`).
    exact_support_edges: HashSet<u32>,
    /// Edges whose supports reproduce the lifted curve (read by `edges`).
    exact_support_curves: HashSet<u32>,
    /// Vertex indices referenced by a transferred edge (read by `vertices`).
    used_vertices: HashSet<usize>,
}

/// Transfer a complete B5 graph. Returns `false` without mutation when any
/// referenced face, pcurve, edge endpoint, or loop chain remains unresolved.
pub(crate) fn transfer(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    mut graph: B5Graph,
    payload: &UnknownId,
) -> bool {
    if !graph.complete {
        graph.loops.retain(|_, loop_| {
            loop_
                .pcurves
                .iter()
                .zip(&loop_.edges)
                .all(|(pcurve, edge)| {
                    (graph
                        .pcurves
                        .get(pcurve)
                        .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                        || graph
                            .opaque_pcurves
                            .get(pcurve)
                            .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                        || graph.implicit_pcurves.get(pcurve) == Some(&loop_.surface))
                        && graph.edge_vertices.contains_key(edge)
                })
                && loop_chain_closes(loop_, &graph.edge_vertices)
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
        let loop_owner_counts = face_loop_owner_counts(&graph.faces);
        graph.faces.retain(|face| {
            face.loops
                .iter()
                .all(|loop_id| loop_owner_counts.get(loop_id).copied() == Some(1))
        });
        let referenced_loops: HashSet<u32> = graph
            .faces
            .iter()
            .flat_map(|face| face.loops.iter().copied())
            .collect();
        graph
            .loops
            .retain(|loop_id, _| referenced_loops.contains(loop_id));
        if graph.faces.is_empty() || graph.loops.is_empty() {
            return false;
        }
        graph.complete = true;
    }
    transfer_complete(ir, annotations, &graph, payload)
}

/// Orchestrate the staged emit passes over a resolved [`TransferPlan`]. The pass
/// order fixes the neutral-model arena and annotation order and must not change.
fn transfer_complete(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    payload: &UnknownId,
) -> bool {
    let Some(mut plan) = build_plan(graph, payload) else {
        return false;
    };
    vertices::emit_vertices(ir, annotations, graph, &plan);
    let surface_ids = surfaces::emit_surfaces(ir, annotations, graph, &mut plan);
    let pcurve_uses = pcurves::emit_pcurves(ir, annotations, graph, &plan);
    let edge_id_map = edges::emit_edges(ir, annotations, graph, payload, &mut plan, &surface_ids);
    if !faces::emit_faces(
        ir,
        annotations,
        graph,
        &plan,
        &surface_ids,
        &pcurve_uses,
        &edge_id_map,
    ) {
        return false;
    }
    true
}

fn referenced_surface_ids(
    roots: impl IntoIterator<Item = u32>,
    offsets: &BTreeMap<u32, B5OffsetSurface>,
    supported: &BTreeMap<u32, B5SupportedSurface>,
    extrusions: &BTreeMap<u32, B5ExtrusionSurface>,
    aliases: &BTreeMap<u32, u32>,
) -> HashSet<u32> {
    let mut referenced = roots.into_iter().collect::<HashSet<_>>();
    let mut pending = referenced.iter().copied().collect::<Vec<_>>();
    while let Some(surface_id) = pending.pop() {
        let Some(construction_id) = super::graph::canonical_surface_id(aliases, surface_id) else {
            continue;
        };
        let dependencies = offsets
            .get(&construction_id)
            .map(|offset| vec![offset.source_surface, offset.carrier_surface])
            .or_else(|| {
                supported.get(&construction_id).map(|construction| {
                    let mut dependencies = construction.support_surfaces.to_vec();
                    dependencies.push(construction.carrier_surface);
                    dependencies
                })
            })
            .or_else(|| {
                extrusions.get(&construction_id).map(|extrusion| {
                    extrusion
                        .directrix
                        .supports()
                        .into_iter()
                        .map(|(support, _, _)| support)
                        .collect()
                })
            })
            .unwrap_or_default();
        for dependency in dependencies {
            if referenced.insert(dependency) {
                pending.push(dependency);
            }
        }
    }
    referenced
}

/// Resolve the whole graph into the cross-pass [`TransferPlan`]. Returns `None`
/// when any referenced surface, pcurve, edge endpoint, or loop chain fails to
/// close so the caller leaves the model untouched.
fn build_plan(graph: &B5Graph, payload: &UnknownId) -> Option<TransferPlan> {
    if graph.faces.is_empty()
        || graph.logical_vertex_refs.len() != graph.logical_vertex_points.len()
    {
        return None;
    }

    let ownership = ownership_plan(graph)?;

    let referenced_surfaces = referenced_surface_ids(
        graph.faces.iter().map(|face| face.surface),
        &graph.offset_surfaces,
        &graph.supported_surfaces,
        &graph.extrusion_surfaces,
        &graph.surface_aliases,
    );
    let mut surface_plan = BTreeMap::new();
    for surface_id in referenced_surfaces {
        let surface = graph.surfaces.get(&surface_id)?;
        surface_plan.insert(
            surface_id,
            surfaces::neutral_surface(surface, graph, surface_id, payload),
        );
    }

    let mut pcurve_plan = BTreeMap::new();
    let mut edge_curve_plan = HashMap::<u32, CurvePlan>::new();
    let mut conflicting_edge_curves = HashSet::<u32>::new();
    let mut edge_helix_plan = HashMap::<u32, HelixPlan>::new();
    let mut edge_support_plan = B5SupportPlan::new();
    let mut loop_senses = BTreeMap::new();
    let mut edge_ids = BTreeSet::new();
    for loop_ in graph.loops.values() {
        if loop_.pcurves.len() != loop_.edges.len() || loop_.pcurves.is_empty() {
            return None;
        }
        let owner = ownership.loop_owners.get(&loop_.object_id).copied()?;
        if graph.faces.get(owner)?.surface != loop_.surface {
            return None;
        }
        if !loop_chain_closes(loop_, &graph.edge_vertices) {
            return None;
        }
        loop_senses.insert(loop_.object_id, loop_.edge_senses());
        for (&pcurve_id, &edge_id) in loop_.pcurves.iter().zip(&loop_.edges) {
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
                            let endpoints = graph.edge_vertices[&edge_id];
                            let Some(points) = endpoints
                                .map(|vertex| b5_vertex_point(graph, vertex))
                                .into_iter()
                                .collect::<Option<Vec<_>>>()
                            else {
                                return false;
                            };
                            circle_contains_points(geometry, &points)
                        })
                    {
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
                        let supports = edge_support_plan.entry(edge_id).or_default();
                        if !supports.iter().any(|(surface, pcurve, range)| {
                            *surface == loop_.surface
                                && *pcurve == pcurve_id
                                && *range == support_range
                        }) {
                            supports.push((loop_.surface, pcurve_id, support_range));
                        }
                        merge_curve_plan(
                            &mut edge_curve_plan,
                            &mut conflicting_edge_curves,
                            edge_id,
                            CurvePlan {
                                geometry,
                                parameter_range: None,
                                edge_tolerance: None,
                                cache_fit_tolerance: None,
                            },
                        );
                    }
                    edge_ids.insert(edge_id);
                    continue;
                }
                if graph.implicit_pcurves.get(&pcurve_id) == Some(&loop_.surface) {
                    edge_ids.insert(edge_id);
                    continue;
                }
                return None;
            };
            if pcurve.surface != loop_.surface || !graph.edge_vertices.contains_key(&edge_id) {
                return None;
            }
            let knots = pcurve_nurbs_knots(pcurve)?;
            let parameter_range = pcurve_parameter_domain(pcurve)?;
            let surface = graph.surfaces.get(&loop_.surface)?;
            let cylinder_reparameterized = matches!(surface, B5Surface::Cylinder { .. });
            let geometry = PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::new(
                    pcurve.degree,
                    knots,
                    pcurve
                        .control_points
                        .iter()
                        .map(|point| neutral_pcurve_point(*point, surface))
                        .collect(),
                    pcurve.weights.clone(),
                    false,
                )
                .ok()?,
            };
            pcurve_plan.entry(pcurve_id).or_insert((
                geometry,
                cylinder_reparameterized,
                parameter_range,
            ));
            let supports = edge_support_plan.entry(edge_id).or_default();
            let support_range = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                .and_then(|parameters| bounded_occurrence_range(parameters, parameter_range))
                .unwrap_or(parameter_range);
            if !supports.iter().any(|(surface, pcurve, range)| {
                *surface == loop_.surface && *pcurve == pcurve_id && *range == support_range
            }) {
                supports.push((loop_.surface, pcurve_id, support_range));
            }
            let lifted = lifted_curve_geometry(pcurve, surface)
                .or_else(|| {
                    let SurfaceGeometry::Nurbs(cache) = &surface_plan.get(&loop_.surface)?.geometry
                    else {
                        return None;
                    };
                    nurbs_isocurve(pcurve, cache).map(CurveGeometry::Nurbs)
                })
                .filter(curve_cache_has_ordered_knots);
            if let Some(geometry) = lifted {
                let endpoints = graph.edge_vertices[&edge_id];
                let (Some(edge_start), Some(edge_end)) = (
                    b5_vertex_point(graph, endpoints[0]),
                    b5_vertex_point(graph, endpoints[1]),
                ) else {
                    return None;
                };
                let oriented_plan = if matches!(surface, B5Surface::Plane { .. }) {
                    edge_pcurve_parameters(graph, edge_id, pcurve_id).and_then(|parameters| {
                        oriented_nurbs_range(geometry.clone(), parameters, edge_start, edge_end)
                    })
                } else if matches!(surface, B5Surface::Nurbs(_) | B5Surface::Revolution { .. }) {
                    edge_pcurve_parameters(graph, edge_id, pcurve_id)
                        .and_then(|parameters| isocurve_endpoint_parameters(pcurve, parameters))
                        .and_then(|parameters| {
                            oriented_nurbs_range(geometry.clone(), parameters, edge_start, edge_end)
                        })
                } else if matches!(geometry, CurveGeometry::Line { .. }) {
                    oriented_line_plan(&geometry, edge_start, edge_end)
                } else if matches!(geometry, CurveGeometry::Circle { .. }) {
                    edge_pcurve_parameters(graph, edge_id, pcurve_id).and_then(|parameters| {
                        oriented_circle_plan(
                            pcurve, surface, &geometry, parameters, edge_start, edge_end,
                        )
                    })
                } else {
                    None
                };
                let plan = oriented_plan.unwrap_or(CurvePlan {
                    geometry,
                    parameter_range: None,
                    edge_tolerance: None,
                    cache_fit_tolerance: None,
                });
                merge_curve_plan(
                    &mut edge_curve_plan,
                    &mut conflicting_edge_curves,
                    edge_id,
                    plan,
                );
                if conflicting_edge_curves.contains(&edge_id) {
                    edge_helix_plan.remove(&edge_id);
                }
            } else {
                let endpoint_indices = graph.edge_vertices[&edge_id];
                let (Some(edge_start), Some(edge_end)) = (
                    b5_vertex_point(graph, endpoint_indices[0]),
                    b5_vertex_point(graph, endpoint_indices[1]),
                ) else {
                    return None;
                };
                let Some(endpoint_parameters) = edge_pcurve_parameters(graph, edge_id, pcurve_id)
                else {
                    edge_ids.insert(edge_id);
                    continue;
                };
                let Some(helix) =
                    cylinder_helix(pcurve, surface, endpoint_parameters, edge_start, edge_end)
                else {
                    edge_ids.insert(edge_id);
                    continue;
                };
                if edge_helix_plan
                    .get(&edge_id)
                    .is_some_and(|existing| existing != &helix)
                {
                    return None;
                }
                merge_curve_plan(
                    &mut edge_curve_plan,
                    &mut conflicting_edge_curves,
                    edge_id,
                    CurvePlan {
                        geometry: CurveGeometry::Nurbs(helix.cache.clone()),
                        parameter_range: Some(helix.parameter_range),
                        edge_tolerance: Some(helix.fit_tolerance),
                        cache_fit_tolerance: Some(helix.fit_tolerance),
                    },
                );
                if conflicting_edge_curves.contains(&edge_id) {
                    edge_helix_plan.remove(&edge_id);
                } else {
                    edge_helix_plan.entry(edge_id).or_insert(helix);
                }
            }
            edge_ids.insert(edge_id);
        }
    }
    let loop_orientation = orient_loop_members(graph, loop_senses)?;
    let vertex_tolerances =
        transfer_vertex_tolerances(graph, &edge_support_plan, &surface_plan, &pcurve_plan);
    for (&edge, supports) in &mut edge_support_plan {
        let vertices = graph.edge_vertices[&edge];
        let [Some(start), Some(end)] = vertices.map(|vertex| b5_vertex_point(graph, vertex)) else {
            continue;
        };
        let tolerances = vertices.map(|vertex| {
            vertex_tolerances
                .get(&vertex)
                .copied()
                .unwrap_or(POINT_TOLERANCE)
                .max(POINT_TOLERANCE)
        });
        orient_b5_supports_to_edge(
            supports,
            [start, end],
            tolerances,
            &surface_plan,
            &pcurve_plan,
        );
    }
    let exact_support_edges = edge_support_plan
        .iter()
        .filter_map(|(&edge, supports)| {
            let vertices = *graph.edge_vertices.get(&edge)?;
            let endpoints = vertices.map(|vertex| b5_vertex_point(graph, vertex));
            let [Some(start), Some(end)] = endpoints else {
                return None;
            };
            let tolerances = vertices.map(|vertex| {
                vertex_tolerances
                    .get(&vertex)
                    .copied()
                    .unwrap_or(POINT_TOLERANCE)
                    .max(POINT_TOLERANCE)
            });
            b5_supports_follow_edge(
                supports,
                [start, end],
                tolerances,
                &surface_plan,
                &pcurve_plan,
            )
            .then_some(edge)
        })
        .collect::<HashSet<_>>();
    let exact_support_curves = edge_support_plan
        .iter()
        .filter_map(|(&edge, supports)| {
            edge_curve_plan
                .get(&edge)
                .map_or_else(
                    || b5_supports_agree(supports, &surface_plan, &pcurve_plan),
                    |plan| b5_supports_follow_curve(supports, plan, &surface_plan, &pcurve_plan),
                )
                .then_some(edge)
        })
        .collect::<HashSet<_>>();

    let used_vertices: HashSet<usize> = edge_ids
        .iter()
        .flat_map(|edge| graph.edge_vertices[edge])
        .collect();

    Some(TransferPlan {
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
    })
}

pub(crate) fn resolved_surface_geometry(
    graph: &B5Graph,
    surface_id: u32,
) -> Option<SurfaceGeometry> {
    let surface = graph.surfaces.get(&surface_id)?;
    let payload = UnknownId("catia:payload:unknown#b5-surface".to_string());
    let geometry = surfaces::neutral_surface(surface, graph, surface_id, &payload).geometry;
    (!matches!(geometry, SurfaceGeometry::Unknown { .. })).then_some(geometry)
}

/// Exact neutral geometry and construction of a surface-of-revolution carrier.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedRevolutionSurface {
    /// Exact NURBS cache of the revolution result.
    pub(crate) geometry: SurfaceGeometry,
    /// Exact profile curve used as the revolution directrix.
    pub(crate) directrix: NurbsCurve,
    /// Point on the revolution axis.
    pub(crate) axis_origin: Point3,
    /// Unit revolution-axis direction.
    pub(crate) axis_direction: Vector3,
    /// Angular interval in radians.
    pub(crate) angular_interval: [f64; 2],
    /// Native angular surface-parameter interval mapped to `angular_interval`.
    pub(crate) angular_parameter_interval: [f64; 2],
    /// Native profile parameter interval.
    pub(crate) parameter_interval: [f64; 2],
}

/// Resolve a surface-of-revolution carrier while retaining its exact
/// procedural construction alongside the cache geometry.
pub(crate) fn resolved_revolution_surface(
    graph: &B5Graph,
    surface_id: u32,
) -> Option<ResolvedRevolutionSurface> {
    let surface = graph.surfaces.get(&surface_id)?;
    let payload = UnknownId("catia:payload:unknown#b5-surface".to_string());
    let SurfacePlan {
        geometry,
        procedure,
    } = surfaces::neutral_surface(surface, graph, surface_id, &payload);
    let SurfaceProcedure::Revolution(plan) = procedure? else {
        return None;
    };
    if !matches!(&geometry, SurfaceGeometry::Nurbs(_)) {
        return None;
    }
    Some(ResolvedRevolutionSurface {
        geometry,
        directrix: plan.directrix,
        axis_origin: plan.axis_origin,
        axis_direction: plan.axis_direction,
        angular_interval: plan.angular_interval,
        angular_parameter_interval: plan.angular_parameter_interval,
        parameter_interval: plan.parameter_interval,
    })
}

#[derive(Clone, PartialEq)]
/// One object-stream pcurve lowered with its exact resolved support carrier.
pub(crate) struct ResolvedObjectStreamPcurve {
    /// Persistent identity of the pcurve's support surface.
    pub(crate) surface_object_id: u32,
    /// Exact neutral support construction.
    pub(crate) carrier: ResolvedPcurveSurface,
    /// Exact neutral parameter-space curve.
    pub(crate) geometry: PcurveGeometry,
    /// Native pcurve parameter interval.
    pub(crate) parameter_range: [f64; 2],
}

/// Exact neutral carrier for an identity-bound object-stream pcurve.
#[derive(Clone, PartialEq)]
pub(crate) enum ResolvedPcurveSurface {
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
pub(crate) fn resolved_surface_carrier(surface: &B5Surface) -> Option<ResolvedPcurveSurface> {
    surfaces::neutral_analytic_surface(surface)
        .map(ResolvedPcurveSurface::Geometry)
        .or_else(|| match surface {
            B5Surface::RollingBall {
                carrier_object_id,
                definition,
            } => Some(ResolvedPcurveSurface::RollingBall {
                carrier_object_id: *carrier_object_id,
                definition: Box::new(definition.clone()),
            }),
            _ => None,
        })
}

/// Resolve a pcurve support carrier with the graph context required by exact
/// constructed surfaces such as surface-of-revolution records.
pub(crate) fn resolved_surface_carrier_in_graph(
    graph: &B5Graph,
    surface_object_id: u32,
) -> Option<ResolvedPcurveSurface> {
    let surface = graph.surfaces.get(&surface_object_id)?;
    resolved_surface_carrier(surface).or_else(|| {
        resolved_surface_geometry(graph, surface_object_id).map(ResolvedPcurveSurface::Geometry)
    })
}

/// Lower one decoded degree-5 UV jet through its resolved native chart.
#[must_use]
pub(crate) fn resolved_object_stream_pcurve(
    pcurve: &crate::families::a5a8::records::A8Pcurve,
    surface: &B5Surface,
    graph: Option<&B5Graph>,
) -> Option<ResolvedObjectStreamPcurve> {
    let carrier = graph
        .and_then(|graph| resolved_surface_carrier_in_graph(graph, pcurve.support_id))
        .or_else(|| resolved_surface_carrier(surface))?;
    let (knots, control_points) = crate::nurbs::quintic_jet_bspline(
        pcurve.degree,
        &pcurve.knots,
        &pcurve.points,
        &pcurve.first_derivatives,
        &pcurve.second_derivatives,
    )?;
    Some(ResolvedObjectStreamPcurve {
        surface_object_id: pcurve.support_id,
        carrier,
        geometry: PcurveGeometry::Nurbs {
            nurbs: PcurveNurbs::new(
                pcurve.degree,
                knots,
                control_points
                    .into_iter()
                    .map(|point| pcurves::neutral_pcurve_point(point, surface))
                    .collect(),
                None,
                false,
            )
            .ok()?,
        },
        parameter_range: pcurve.range,
    })
}

pub(crate) fn resolved_surface_procedural_definition(
    graph: &B5Graph,
    surface_id: u32,
) -> Option<(u32, ProceduralSurfaceDefinition)> {
    let surface = graph.surfaces.get(&surface_id)?;
    let payload = UnknownId("catia:payload:unknown#b5-surface".to_string());
    match surfaces::neutral_surface(surface, graph, surface_id, &payload).procedure? {
        SurfaceProcedure::RollingBall {
            carrier_object_id,
            definition,
        } => Some((carrier_object_id, definition)),
        SurfaceProcedure::Extrusion(_) | SurfaceProcedure::Revolution(_) => None,
    }
}

/// Neutral support evidence for one side of an exact extrusion directrix.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedExtrusionSupport {
    /// Persistent support-surface identity.
    pub(crate) surface_object_id: u32,
    /// Exact neutral support geometry.
    pub(crate) surface: SurfaceGeometry,
    /// Exact parameter-space directrix occurrence.
    pub(crate) pcurve: PcurveGeometry,
    /// Native interval used by this support occurrence.
    pub(crate) pcurve_parameter_range: [f64; 2],
    /// Exact model-space lift when the support chart admits one.
    pub(crate) curve: Option<CurveGeometry>,
}

/// Exact neutral construction of one extrusion directrix.
#[derive(Clone, PartialEq)]
pub(crate) enum ResolvedExtrusionDirectrix {
    /// Intersection of two support surfaces.
    Intersection {
        /// Ordered exact support sides.
        supports: Box<[ResolvedExtrusionSupport; 2]>,
        /// Positive fit tolerance of the retained sampled cache.
        cache_fit_tolerance: f64,
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
        source_parameter_range: [f64; 2],
        /// Signed offset distance.
        distance: f64,
        /// Unit direction defining the positive offset side.
        direction: Vector3,
    },
}

/// Exact two-support directrix and extrusion chart resolved from B5 objects.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedExtrusionSurface {
    /// Persistent extrusion-surface identity.
    pub(crate) surface_object_id: u32,
    /// Persistent directrix identity.
    pub(crate) directrix_object_id: u32,
    /// Solved directrix interval shared by the support mappings.
    pub(crate) directrix_parameter_range: [f64; 2],
    /// Unit world-space extrusion direction.
    pub(crate) direction: Vector3,
    /// Ordered native U and V chart bounds.
    pub(crate) parameter_bounds: [[f64; 2]; 2],
    /// Exact directrix construction.
    pub(crate) directrix: ResolvedExtrusionDirectrix,
}

impl ResolvedExtrusionSurface {
    pub(crate) fn supports(&self) -> Vec<&ResolvedExtrusionSupport> {
        match &self.directrix {
            ResolvedExtrusionDirectrix::Intersection { supports, .. } => supports.iter().collect(),
            ResolvedExtrusionDirectrix::SurfaceCurve { support, .. }
            | ResolvedExtrusionDirectrix::Offset { support, .. } => vec![support],
        }
    }
}

/// Exact support construction of a resolved offset surface.
#[derive(Clone, PartialEq)]
pub(crate) enum ResolvedOffsetSupport {
    /// Direct neutral support geometry.
    Geometry(SurfaceGeometry),
    /// Procedural extrusion support.
    Extrusion(Box<ResolvedExtrusionSurface>),
}

/// Exact offset construction resolved from a B5 class-`30` object.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedOffsetSurface {
    /// Persistent result-carrier identity.
    pub(crate) carrier_object_id: u32,
    /// Persistent support-surface identity.
    pub(crate) support_object_id: u32,
    /// Exact support construction.
    pub(crate) support: ResolvedOffsetSupport,
    /// Signed offset distance.
    pub(crate) distance: f64,
    /// Ordered native U and V chart bounds.
    pub(crate) parameter_bounds: [[f64; 2]; 2],
}

pub(crate) fn resolved_extrusion_surface(
    graph: &B5Graph,
    surface_id: u32,
) -> Option<ResolvedExtrusionSurface> {
    let construction_id = graph.canonical_surface_id(surface_id)?;
    let extrusion = graph.extrusion_surfaces.get(&construction_id)?;
    let resolve_support =
        |(surface_object_id, pcurve_object_id, pcurve_parameter_range): (u32, u32, [f64; 2])| {
            let source_surface = graph.surfaces.get(&surface_object_id)?;
            let surface = resolved_surface_geometry(graph, surface_object_id)?;
            let pcurve = graph.pcurves.get(&pcurve_object_id)?;
            let knots = pcurve_nurbs_knots(pcurve)?;
            let domain = pcurve_parameter_domain(pcurve)?;
            bounded_occurrence_range(pcurve_parameter_range, domain)?;
            let pcurve_geometry = PcurveGeometry::Nurbs {
                nurbs: PcurveNurbs::new(
                    pcurve.degree,
                    knots,
                    pcurve
                        .control_points
                        .iter()
                        .map(|point| neutral_pcurve_point(*point, source_surface))
                        .collect(),
                    pcurve.weights.clone(),
                    false,
                )
                .ok()?,
            };
            let curve = lifted_curve_geometry(pcurve, source_surface);
            Some(ResolvedExtrusionSupport {
                surface_object_id,
                surface,
                pcurve: pcurve_geometry,
                pcurve_parameter_range,
                curve,
            })
        };
    let directrix = match &extrusion.directrix {
        B5ExtrusionDirectrix::Intersection {
            supports,
            cache_fit_tolerance,
            ..
        } => {
            let supports: [ResolvedExtrusionSupport; 2] = supports
                .map(resolve_support)
                .into_iter()
                .collect::<Option<Vec<_>>>()?
                .try_into()
                .ok()?;
            (supports[0].surface_object_id != supports[1].surface_object_id).then_some(())?;
            ResolvedExtrusionDirectrix::Intersection {
                supports: Box::new(supports),
                cache_fit_tolerance: *cache_fit_tolerance,
            }
        }
        B5ExtrusionDirectrix::SurfaceCurve { support, .. } => {
            let support = resolve_support(*support)?;
            let curve = curve_on_parameter_range(
                support.curve.clone()?,
                support.pcurve_parameter_range,
                extrusion.parameter_bounds[1],
            )?;
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
            let support = resolve_support(*support)?;
            let source_curve = curve_on_parameter_range(
                support.curve.clone()?,
                *source_parameter_range,
                extrusion.parameter_bounds[1],
            )?;
            ResolvedExtrusionDirectrix::Offset {
                source_object_id: *object_id,
                support,
                source_curve,
                source_parameter_range: extrusion.parameter_bounds[1],
                distance: *distance,
                direction: vector(*direction),
            }
        }
    };
    Some(ResolvedExtrusionSurface {
        surface_object_id: surface_id,
        directrix_object_id: extrusion.directrix.object_id(),
        directrix_parameter_range: extrusion.parameter_bounds[1],
        direction: vector(extrusion.direction),
        parameter_bounds: extrusion.parameter_bounds,
        directrix,
    })
}

fn curve_on_parameter_range(
    curve: CurveGeometry,
    source: [f64; 2],
    target: [f64; 2],
) -> Option<CurveGeometry> {
    if parameter_range_contains(source, target) {
        return Some(curve);
    }
    let source_span = source[1] - source[0];
    let target_span = target[1] - target[0];
    if !source_span.is_finite()
        || source_span <= 0.0
        || !target_span.is_finite()
        || target_span <= 0.0
    {
        return None;
    }
    let target_per_source = target_span / source_span;
    let source_per_target = source_span / target_span;
    match curve {
        CurveGeometry::Nurbs(mut curve) => {
            for knot in curve.knots_mut() {
                *knot = target[0] + (*knot - source[0]) * target_per_source;
            }
            Some(CurveGeometry::Nurbs(curve))
        }
        CurveGeometry::Line { origin, direction } => Some(CurveGeometry::Line {
            origin: Point3::new(
                origin.x + (source[0] - target[0] * source_per_target) * direction.x,
                origin.y + (source[0] - target[0] * source_per_target) * direction.y,
                origin.z + (source[0] - target[0] * source_per_target) * direction.z,
            ),
            direction: Vector3::new(
                direction.x * source_per_target,
                direction.y * source_per_target,
                direction.z * source_per_target,
            ),
        }),
        _ => None,
    }
}

fn parameter_range_contains(domain: [f64; 2], active: [f64; 2]) -> bool {
    let scale = domain
        .into_iter()
        .chain(active)
        .map(f64::abs)
        .fold(1.0f64, f64::max);
    let tolerance = 64.0 * f64::EPSILON * scale;
    domain[0] <= active[0] + tolerance && active[1] <= domain[1] + tolerance
}

pub(crate) fn resolved_offset_surface(
    graph: &B5Graph,
    surface_id: u32,
) -> Option<ResolvedOffsetSurface> {
    let construction_id = graph.canonical_surface_id(surface_id)?;
    let offset = graph.offset_surfaces.get(&construction_id)?;
    let support = resolved_surface_geometry(graph, offset.source_surface)
        .map(ResolvedOffsetSupport::Geometry)
        .or_else(|| {
            resolved_extrusion_surface(graph, offset.source_surface)
                .map(Box::new)
                .map(ResolvedOffsetSupport::Extrusion)
        })?;
    Some(ResolvedOffsetSurface {
        carrier_object_id: offset.carrier_surface,
        support_object_id: offset.source_surface,
        support,
        distance: offset.distance,
        parameter_bounds: offset.parameter_bounds,
    })
}

fn annotate(
    annotations: &mut AnnotationBuilder,
    id: impl std::fmt::Display,
    stream: &str,
    tag: &str,
    exactness: Exactness,
) {
    let id = id.to_string();
    let stream = annotations.stream(format!("catia:{stream}"));
    annotations.note(&id, stream, 0).tag(tag);
    annotations.exactness(id, exactness);
}

fn point(value: [f64; 3]) -> Point3 {
    Point3::new(value[0], value[1], value[2])
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

fn distance(left: [f64; 3], right: [f64; 3]) -> f64 {
    left.into_iter()
        .zip(right)
        .map(|(left, right)| (left - right) * (left - right))
        .sum::<f64>()
        .sqrt()
}

fn circle_contains_points(geometry: &CurveGeometry, points: &[[f64; 3]]) -> bool {
    let CurveGeometry::Circle {
        center,
        axis,
        radius,
        ..
    } = geometry
    else {
        return false;
    };
    let center = [center.x, center.y, center.z];
    let axis = [axis.x, axis.y, axis.z];
    points.iter().all(|point| {
        let offset = subtract(*point, center);
        (length(offset) - radius).abs() <= POINT_TOLERANCE
            && dot(offset, axis).abs() <= POINT_TOLERANCE
    })
}

// `unit` normalizes by per-component division, a bit-level-distinct form from
// the parse graph's reciprocal-multiply (`graph::unit`). The two must NOT be
// unified: the affected profiles depend on the exact rounding of each form.
fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = value[0].hypot(value[1]).hypot(value[2]);
    (length.is_finite() && length != 0.0)
        .then(|| [value[0] / length, value[1] / length, value[2] / length])
}
#[cfg(test)]
mod tests;
