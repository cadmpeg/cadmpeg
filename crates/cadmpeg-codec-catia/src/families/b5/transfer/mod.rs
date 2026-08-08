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
    CurveGeometry, NurbsCurve, PcurveGeometry, ProceduralCurveDefinition,
    ProceduralSurfaceDefinition, SurfaceGeometry,
};
use cadmpeg_ir::ids::UnknownId;
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::BodyKind;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::graph::{
    bounded_occurrence_range, edge_pcurve_parameters, face_loop_owner_counts, loop_chain_closes,
    B5ExtrusionDirectrix, B5ExtrusionSurface, B5Graph, B5OffsetSurface, B5SupportedSurface,
    B5Surface,
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

const POINT_TOLERANCE: f64 = 1.5e-3;

type B5Support = (u32, u32, [f64; 2]);
type B5SupportPlan = HashMap<u32, Vec<B5Support>>;

struct RevolutionPlan {
    directrix: NurbsCurve,
    axis_origin: Point3,
    axis_direction: Vector3,
    angular_interval: [f64; 2],
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
    faces::emit_faces(
        ir,
        annotations,
        graph,
        &plan,
        &surface_ids,
        &pcurve_uses,
        &edge_id_map,
    );
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

fn native_pcurve_parameter_range(
    pcurve: &super::graph::B5Pcurve,
    knots: &[f64],
) -> Option<[f64; 2]> {
    let degree = usize::try_from(pcurve.degree).ok()?;
    let spline_domain = knots
        .get(degree)
        .copied()
        .zip(
            knots
                .len()
                .checked_sub(degree + 1)
                .and_then(|index| knots.get(index))
                .copied(),
        )
        .map(|(start, end)| [start, end])
        .filter(|range| range[0].is_finite() && range[0] < range[1])?;
    match pcurve.parameter_range {
        Some(range) => bounded_occurrence_range(range, spline_domain),
        None => Some(spline_domain),
    }
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
        if graph
            .faces
            .iter()
            .filter(|face| face.loops.contains(&loop_.object_id))
            .any(|face| face.surface != loop_.surface)
        {
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
            let knots = expand_knots(&pcurve.distinct_knots, &pcurve.multiplicities)?;
            let parameter_range = native_pcurve_parameter_range(pcurve, &knots)?;
            let surface = graph.surfaces.get(&loop_.surface)?;
            let cylinder_reparameterized = matches!(surface, B5Surface::Cylinder { .. });
            let geometry = PcurveGeometry::Nurbs {
                degree: pcurve.degree,
                knots,
                control_points: pcurve
                    .control_points
                    .iter()
                    .map(|point| neutral_pcurve_point(*point, surface))
                    .collect(),
                weights: pcurve.weights.clone(),
                periodic: false,
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
            degree: pcurve.degree,
            knots,
            control_points: control_points
                .into_iter()
                .map(|point| pcurves::neutral_pcurve_point(point, surface))
                .collect(),
            weights: None,
            periodic: false,
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
            let knots = expand_knots(&pcurve.distinct_knots, &pcurve.multiplicities)?;
            let degree = usize::try_from(pcurve.degree).ok()?;
            let domain = [
                *knots.get(degree)?,
                *knots.get(knots.len().checked_sub(degree + 1)?)?,
            ];
            bounded_occurrence_range(pcurve_parameter_range, domain)?;
            let pcurve_geometry = PcurveGeometry::Nurbs {
                degree: pcurve.degree,
                knots,
                control_points: pcurve
                    .control_points
                    .iter()
                    .map(|point| neutral_pcurve_point(*point, source_surface))
                    .collect(),
                weights: pcurve.weights.clone(),
                periodic: false,
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
            for knot in &mut curve.knots {
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

fn expand_knots(distinct: &[f64], multiplicities: &[u32]) -> Option<Vec<f64>> {
    if distinct.len() != multiplicities.len() {
        return None;
    }
    let mut knots = Vec::new();
    for (&knot, &multiplicity) in distinct.iter().zip(multiplicities) {
        knots.extend(std::iter::repeat_n(
            knot,
            usize::try_from(multiplicity).ok()?,
        ));
    }
    Some(knots)
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
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn dot(left: [f64; 3], right: [f64; 3]) -> f64 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn length(value: [f64; 3]) -> f64 {
    dot(value, value).sqrt()
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
mod tests {
    use super::super::graph::{
        bounded_occurrence_range, edge_pcurve_parameters, loop_chain_closes, B5ExtrusionDirectrix,
        B5ExtrusionSurface, B5Face, B5Graph, B5Loop, B5LoopMetadata, B5OffsetSurface,
        B5OpaquePcurve, B5ParameterIncidence, B5Pcurve, B5Profile, B5SphereGreatCirclePcurve,
        B5SupportedSurface, B5SupportedSurfaceParameters, B5Surface,
    };
    use super::edges::{
        b5_edge_support_definition, b5_supports_follow_edge, curve_cache_has_ordered_knots,
        merge_curve_plan, ordered_subrange, orient_b5_supports_to_edge,
    };
    use super::faces::{orient_loop_members, ownership_plan};
    use super::pcurves::{
        cylinder_helix, cylinder_point, isocurve_endpoint_parameters, lifted_curve_geometry,
        neutral_pcurve_point, oriented_circle_plan, oriented_line_plan, oriented_nurbs_range,
        sphere_great_circle_geometry, sphere_great_circle_pcurve,
    };
    use super::unit;

    #[test]
    fn unit_preserves_tiny_finite_direction() {
        assert_eq!(unit([1e-200, 0.0, 0.0]), Some([1.0, 0.0, 0.0]));
        assert_eq!(unit([0.0, 0.0, 0.0]), None);
    }
    use super::surfaces::{rational_arc, revolution_surface, revolve_nurbs};
    use super::vertices::transfer_vertex_tolerances;
    use super::{
        build_plan, curve_on_parameter_range, native_pcurve_parameter_range,
        referenced_surface_ids, resolved_surface_carrier_in_graph, transfer, CurvePlan,
        ResolvedPcurveSurface, SurfacePlan,
    };
    use cadmpeg_ir::document::CadIr;
    use cadmpeg_ir::eval::surface_point;
    use cadmpeg_ir::geometry::{
        CurveGeometry, NurbsCurve, PcurveGeometry, ProceduralCurveDefinition, SurfaceGeometry,
    };
    use cadmpeg_ir::ids::{SurfaceId, UnknownId};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use cadmpeg_ir::topology::BodyKind;
    use cadmpeg_ir::units::Units;
    use cadmpeg_ir::AnnotationBuilder;
    use std::collections::{BTreeMap, HashMap, HashSet};

    #[test]
    fn affine_curve_ranges_reparameterize_without_changing_geometry() {
        let nurbs = NurbsCurve {
            degree: 1,
            knots: vec![10.0, 10.0, 20.0, 20.0],
            control_points: vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)],
            weights: None,
            periodic: false,
        };
        let CurveGeometry::Nurbs(translated) = curve_on_parameter_range(
            CurveGeometry::Nurbs(nurbs.clone()),
            [10.0, 20.0],
            [0.0, 10.0],
        )
        .expect("equal-span NURBS translation") else {
            unreachable!();
        };
        assert_eq!(translated.knots, [0.0, 0.0, 10.0, 10.0]);
        assert_eq!(translated.control_points, nurbs.control_points);

        let line = CurveGeometry::Line {
            origin: Point3::new(10.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        };
        assert_eq!(
            curve_on_parameter_range(line, [10.0, 20.0], [0.0, 10.0]),
            Some(CurveGeometry::Line {
                origin: Point3::new(20.0, 0.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            })
        );
        assert_eq!(
            curve_on_parameter_range(
                CurveGeometry::Nurbs(nurbs.clone()),
                [10.0, 20.0],
                [12.0, 18.0],
            ),
            Some(CurveGeometry::Nurbs(nurbs.clone()))
        );
        let CurveGeometry::Nurbs(scaled) =
            curve_on_parameter_range(CurveGeometry::Nurbs(nurbs), [10.0, 20.0], [0.0, 2.0])
                .expect("positive affine NURBS mapping")
        else {
            unreachable!();
        };
        assert_eq!(scaled.knots, [0.0, 0.0, 2.0, 2.0]);
        assert_eq!(
            curve_on_parameter_range(
                CurveGeometry::Line {
                    origin: Point3::new(10.0, 0.0, 0.0),
                    direction: Vector3::new(1.0, 0.0, 0.0),
                },
                [10.0, 20.0],
                [0.0, 2.0],
            ),
            Some(CurveGeometry::Line {
                origin: Point3::new(20.0, 0.0, 0.0),
                direction: Vector3::new(5.0, 0.0, 0.0),
            })
        );
    }

    #[test]
    fn explicit_pcurve_range_must_be_a_subrange_of_its_knot_domain() {
        let mut pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 10.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [1.0, 0.0]],
            weights: None,
            parameter_range: Some([2.0, 8.0]),
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        let knots = vec![0.0, 0.0, 10.0, 10.0];
        assert_eq!(
            native_pcurve_parameter_range(&pcurve, &knots),
            Some([2.0, 8.0])
        );
        pcurve.parameter_range = None;
        assert_eq!(
            native_pcurve_parameter_range(&pcurve, &knots),
            Some([0.0, 10.0])
        );
        pcurve.parameter_range = Some([-1.0, 8.0]);
        assert_eq!(native_pcurve_parameter_range(&pcurve, &knots), None);
    }

    fn test_loop_metadata(edge_count: usize) -> B5LoopMetadata {
        B5LoopMetadata {
            framing_controls: [0x05, 0x05],
            edge_controls: vec![[1, 1, 1]; edge_count],
            extension: None,
        }
    }

    #[test]
    fn support_bound_surface_closure_includes_carrier_supports_and_offsets() {
        let offsets = BTreeMap::from([(
            30,
            B5OffsetSurface {
                object_id: 30,
                carrier_surface: 31,
                source_surface: 50,
                distance: 1.0,
                carrier_kind: 2,
                parameter_bounds: [[0.0, 1.0], [0.0, 1.0]],
            },
        )]);
        let supported = BTreeMap::from([(
            10,
            B5SupportedSurface {
                object_id: 10,
                carrier_surface: 20,
                support_surfaces: [30, 40],
                support_pcurves: [60, 70],
                parameters: B5SupportedSurfaceParameters::Radius {
                    controls: [1; 6],
                    construction_radius: 2.0,
                },
            },
        )]);
        let extrusions = BTreeMap::from([(
            50,
            B5ExtrusionSurface {
                object_id: 50,
                direction: [0.0, 0.0, 1.0],
                parameter_bounds: [[0.0, 1.0], [0.0, 2.0]],
                directrix: B5ExtrusionDirectrix::Intersection {
                    object_id: 80,
                    supports: [(90, 91, [0.0, 1.0]), (100, 101, [0.0, 1.0])],
                    parameter_range: [0.0, 1.0],
                    cache_fit_tolerance: 1e-6,
                },
            },
        )]);

        assert_eq!(
            referenced_surface_ids([10], &offsets, &supported, &extrusions, &BTreeMap::new(),),
            HashSet::from([10, 20, 30, 31, 40, 50, 90, 100])
        );
    }

    #[test]
    fn surface_closure_follows_aliases_to_native_constructions() {
        let offsets = BTreeMap::from([(
            20,
            B5OffsetSurface {
                object_id: 20,
                carrier_surface: 30,
                source_surface: 40,
                distance: 2.0,
                carrier_kind: 2,
                parameter_bounds: [[0.0, 1.0], [0.0, 2.0]],
            },
        )]);
        let aliases = BTreeMap::from([(10, 11), (11, 20)]);

        assert_eq!(
            referenced_surface_ids([10], &offsets, &BTreeMap::new(), &BTreeMap::new(), &aliases,),
            HashSet::from([10, 30, 40])
        );
    }

    #[test]
    fn occurrence_interval_orders_and_bounds_native_stations() {
        assert_eq!(ordered_subrange([8.0, 2.0], [0.0, 10.0]), Some([2.0, 8.0]));
        assert_eq!(
            ordered_subrange([-5e-10, 10.0 + 5e-10], [0.0, 10.0]),
            Some([0.0, 10.0])
        );
        assert!(ordered_subrange([2.0, 2.0], [0.0, 10.0]).is_none());
        assert!(ordered_subrange([-2e-9, 8.0], [0.0, 10.0]).is_none());
        assert!(ordered_subrange([2.0, 12.0], [0.0, 10.0]).is_none());
        assert_eq!(
            bounded_occurrence_range([8.0, 2.0], [0.0, 10.0]),
            Some([8.0, 2.0])
        );

        let tiny = 1e-200_f64;
        assert_eq!(
            bounded_occurrence_range([0.0, tiny], [0.0, tiny]),
            Some([0.0, tiny])
        );
        assert!(bounded_occurrence_range([0.0, 2.0 * tiny], [0.0, tiny]).is_none());
        assert!(bounded_occurrence_range([0.0, tiny], [tiny, 0.0]).is_none());
    }

    #[test]
    fn edge_parameters_follow_ordered_edge_refs_for_a_closed_vertex() {
        let mut graph = B5Graph {
            complete: false,
            faces: Vec::new(),
            face_records: BTreeMap::new(),
            loops: BTreeMap::new(),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (
                    40,
                    B5ParameterIncidence {
                        object_id: 40,
                        curves: vec![20],
                        parameters: vec![0.0],
                        controls: vec![0],
                    },
                ),
                (
                    41,
                    B5ParameterIncidence {
                        object_id: 41,
                        curves: vec![20],
                        parameters: vec![1.0],
                        controls: vec![0],
                    },
                ),
            ]),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[0.0, 0.0, 0.0]],
            logical_vertex_refs: vec![50],
            edge_vertices: BTreeMap::from([(30, [0, 0])]),
            edge_parameter_incidences: BTreeMap::from([(30, [40, 41])]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };

        assert_eq!(edge_pcurve_parameters(&graph, 30, 20), Some([0.0, 1.0]));
        graph.edge_parameter_incidences.insert(30, [41, 40]);
        assert_eq!(edge_pcurve_parameters(&graph, 30, 20), Some([1.0, 0.0]));
    }

    /// An incomplete graph keeps the face whose loop members all carry vertex
    /// loci and excludes the face whose members carry none, so that a carrier
    /// without recoverable geometry cannot pull invented vertices into the
    /// neutral model.
    #[test]
    fn incomplete_graph_excludes_a_face_whose_members_have_no_vertex_loci() {
        let plane = |v_offset: f64| B5Surface::Plane {
            origin: [0.0, v_offset, 0.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let line_pcurve = |object_id: u32, surface: u32| B5Pcurve {
            object_id,
            surface,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [1.0, 0.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        let incidence = |object_id: u32, curve: u32, parameter: f64| B5ParameterIncidence {
            object_id,
            curves: vec![curve],
            parameters: vec![parameter],
            controls: vec![0],
        };
        let graph = B5Graph {
            complete: false,
            faces: vec![
                B5Face {
                    object_id: 1,
                    surface: 10,
                    loops: vec![2],
                    terminal_control: None,
                },
                B5Face {
                    object_id: 3,
                    surface: 11,
                    loops: vec![4],
                    terminal_control: None,
                },
            ],
            face_records: BTreeMap::new(),
            loops: BTreeMap::from([
                (
                    2,
                    B5Loop {
                        object_id: 2,
                        pcurves: vec![20, 20, 20],
                        edges: vec![30, 31, 32],
                        metadata: test_loop_metadata(3),
                        surface: 10,
                    },
                ),
                (
                    4,
                    B5Loop {
                        object_id: 4,
                        pcurves: vec![21, 21, 21],
                        edges: vec![33, 34, 35],
                        metadata: test_loop_metadata(3),
                        surface: 11,
                    },
                ),
            ]),
            pcurves: BTreeMap::from([(20, line_pcurve(20, 10)), (21, line_pcurve(21, 11))]),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::from([(10, plane(0.0)), (11, plane(5.0))]),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (40, incidence(40, 20, 0.0)),
                (41, incidence(41, 20, 0.5)),
                (42, incidence(42, 20, 1.0)),
            ]),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_refs: vec![50, 51, 52],
            // Edges 33, 34, and 35 have no entry: their carrier resolves no
            // endpoint locus, which is what excludes face 3.
            edge_vertices: BTreeMap::from([(30, [0, 1]), (31, [1, 2]), (32, [2, 0])]),
            edge_parameter_incidences: BTreeMap::from([
                (30, [40, 41]),
                (31, [41, 42]),
                (32, [42, 40]),
            ]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let mut ir = CadIr::empty(Units::default());

        assert!(transfer(
            &mut ir,
            &mut AnnotationBuilder::new(),
            graph,
            &UnknownId("catia:test-payload".to_string()),
        ));
        assert_eq!(
            ir.model
                .faces
                .iter()
                .map(|face| face.id.0.as_str())
                .collect::<Vec<_>>(),
            ["catia:b5:face#1"]
        );
        assert_eq!(
            ir.model
                .loops
                .iter()
                .map(|loop_| loop_.id.0.as_str())
                .collect::<Vec<_>>(),
            ["catia:b5:loop#2"]
        );
        assert_eq!(ir.model.coedges.len(), 3);
        assert!(!ir
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id.0.contains("#11")));
    }

    #[test]
    fn repeated_source_pcurve_retains_occurrence_ranges_and_directions() {
        let mut graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
                terminal_control: None,
            }],
            face_records: BTreeMap::new(),
            loops: BTreeMap::from([(
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![20, 20, 20],
                    edges: vec![30, 31, 32],
                    metadata: test_loop_metadata(3),
                    surface: 10,
                },
            )]),
            pcurves: BTreeMap::from([(
                20,
                B5Pcurve {
                    object_id: 20,
                    surface: 10,
                    degree: 1,
                    distinct_knots: vec![0.0, 1.0],
                    multiplicities: vec![2, 2],
                    control_points: vec![[0.0, 0.0], [1.0, 0.0]],
                    weights: None,
                    parameter_range: None,
                    class_21_suffix_scalar: None,
                    lifted_endpoints: None,
                },
            )]),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::from([(
                10,
                B5Surface::Plane {
                    origin: [0.0, 0.0, 0.0],
                    direction_u: [1.0, 0.0, 0.0],
                    direction_v: [0.0, 1.0, 0.0],
                    u_range: [-1.0, 1.0],
                    v_range: [-1.0, 1.0],
                },
            )]),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (
                    40,
                    B5ParameterIncidence {
                        object_id: 40,
                        curves: vec![20],
                        parameters: vec![0.0],
                        controls: vec![0],
                    },
                ),
                (
                    41,
                    B5ParameterIncidence {
                        object_id: 41,
                        curves: vec![20],
                        parameters: vec![0.5],
                        controls: vec![0],
                    },
                ),
                (
                    42,
                    B5ParameterIncidence {
                        object_id: 42,
                        curves: vec![20],
                        parameters: vec![1.0],
                        controls: vec![0],
                    },
                ),
            ]),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[0.0, 0.0, 0.0], [0.5, 0.0, 0.0], [1.0, 0.0, 0.0]],
            logical_vertex_refs: vec![50, 51, 52],
            edge_vertices: BTreeMap::from([(30, [0, 1]), (31, [1, 2]), (32, [2, 0])]),
            edge_parameter_incidences: BTreeMap::from([
                (30, [40, 41]),
                (31, [41, 42]),
                (32, [42, 40]),
            ]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        graph
            .loops
            .get_mut(&2)
            .expect("required loop")
            .metadata
            .edge_controls[1][2] = -1;
        let mut ir = CadIr::empty(Units::default());

        assert!(transfer(
            &mut ir,
            &mut AnnotationBuilder::new(),
            graph,
            &UnknownId("catia:test-payload".to_string()),
        ));
        assert_eq!(ir.model.pcurves.len(), 3);
        assert_eq!(ir.model.coedges.len(), 3);
        assert_eq!(
            ir.model
                .points
                .iter()
                .map(|point| point
                    .source_object
                    .as_ref()
                    .map(|source| source.object_id.as_str()))
                .collect::<Vec<_>>(),
            [
                Some("cgm-vertex:000032"),
                Some("cgm-vertex:000033"),
                Some("cgm-vertex:000034"),
            ]
        );
        assert_eq!(
            ir.model
                .pcurves
                .iter()
                .map(|pcurve| pcurve.parameter_range)
                .collect::<Vec<_>>(),
            [Some([0.0, 0.5]), Some([0.0, 1.0]), Some([0.5, 1.0])]
        );
        assert_eq!(
            ir.model
                .coedges
                .iter()
                .flat_map(|coedge| coedge.pcurves.iter().map(|use_| use_.pcurve.0.as_str()))
                .collect::<Vec<_>>(),
            [
                "catia:b5:pcurve#20@0",
                "catia:b5:pcurve#20@2",
                "catia:b5:pcurve#20@1",
            ]
        );
        assert_eq!(
            ir.model
                .coedges
                .iter()
                .map(|coedge| coedge.pcurves[0].parameter_range)
                .collect::<Vec<_>>(),
            [None, Some([1.0, 0.5]), None]
        );
        assert_eq!(ir.model.loops.len(), 1);
        assert_eq!(
            ir.model.loops[0]
                .vertex_uses
                .iter()
                .map(|use_| use_.vertex.0.as_str())
                .collect::<Vec<_>>(),
            [
                "catia:b5:vertex#1",
                "catia:b5:vertex#2",
                "catia:b5:vertex#0"
            ]
        );
        assert_eq!(
            ir.model.loops[0]
                .vertex_uses
                .iter()
                .map(|use_| use_.after.as_ref().map(|coedge| coedge.0.as_str()))
                .collect::<Vec<_>>(),
            [
                Some("catia:b5:coedge#2-0"),
                Some("catia:b5:coedge#2-1"),
                Some("catia:b5:coedge#2-2")
            ]
        );
    }

    #[test]
    fn edge_supports_preserve_one_sided_and_intersection_constructions() {
        let surfaces = HashMap::from([
            (10, SurfaceId("surface-10".to_string())),
            (11, SurfaceId("surface-11".to_string())),
        ]);
        let pcurve_20 = PcurveGeometry::Line {
            origin: Point2::new(0.0, 0.0),
            direction: Point2::new(1.0, 0.0),
        };
        let pcurve_21 = PcurveGeometry::Line {
            origin: Point2::new(0.0, 1.0),
            direction: Point2::new(1.0, 0.0),
        };
        let pcurves = BTreeMap::from([
            (20, (pcurve_20.clone(), false, [2.0, 4.0])),
            (21, (pcurve_21.clone(), false, [2.0, 5.0])),
        ]);
        let (_, _, one_sided) =
            b5_edge_support_definition(&[(10, 20, [2.0, 4.0])], &surfaces, &pcurves, None)
                .expect("one-sided surface curve");
        assert!(matches!(
            one_sided,
            ProceduralCurveDefinition::SurfaceCurve { context, .. }
                if context.parameter_range == [2.0, 4.0]
                    && context.sides[0].surface == Some(surfaces[&10].clone())
                    && context.sides[0].pcurve == Some(pcurve_20)
                    && context.sides[1].surface.is_none()
        ));

        let (_, _, intersection) = b5_edge_support_definition(
            &[(10, 20, [2.0, 4.0]), (11, 21, [2.0, 4.0])],
            &surfaces,
            &pcurves,
            None,
        )
        .expect("two-sided intersection");
        assert!(matches!(
            intersection,
            ProceduralCurveDefinition::Intersection { context, .. }
                if context.parameter_range == [2.0, 4.0]
                    && context.sides[1].surface == Some(surfaces[&11].clone())
                    && context.sides[1].pcurve == Some(pcurve_21)
                    && context.sides.iter().all(|side| side.pcurve_parameter_range.is_none())
        ));
        let (_, _, independently_parameterized) = b5_edge_support_definition(
            &[(10, 20, [2.0, 4.0]), (11, 21, [5.0, 2.0])],
            &surfaces,
            &pcurves,
            None,
        )
        .expect("independently parameterized intersection");
        assert!(matches!(
            independently_parameterized,
            ProceduralCurveDefinition::Intersection { context, .. }
                if context.parameter_range == [0.0, 1.0]
                    && context.sides[0].pcurve_parameter_range == Some([2.0, 4.0])
                    && context.sides[1].pcurve_parameter_range == Some([5.0, 2.0])
        ));
        let (_, _, distance_parameterized) = b5_edge_support_definition(
            &[(10, 20, [2.0, 4.0])],
            &surfaces,
            &pcurves,
            Some([0.0, 8.0]),
        )
        .expect("distance-parameterized surface curve");
        assert!(matches!(
            distance_parameterized,
            ProceduralCurveDefinition::SurfaceCurve { context, .. }
                if context.parameter_range == [0.0, 8.0]
                    && context.sides[0].pcurve_parameter_range == Some([2.0, 4.0])
        ));
    }

    #[test]
    fn procedural_support_requires_physical_edge_endpoint_agreement() {
        let surfaces = BTreeMap::from([(
            10,
            SurfacePlan {
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                procedure: None,
            },
        )]);
        let pcurves = BTreeMap::from([(
            20,
            (
                PcurveGeometry::Line {
                    origin: Point2::new(0.0, 0.0),
                    direction: Point2::new(1.0, 0.0),
                },
                false,
                [0.0, 1.0],
            ),
        )]);
        let supports = [(10, 20, [0.0, 1.0])];
        assert!(b5_supports_follow_edge(
            &supports,
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            [1.5e-3; 2],
            &surfaces,
            &pcurves,
        ));
        assert!(!b5_supports_follow_edge(
            &supports,
            [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            [1.5e-3; 2],
            &surfaces,
            &pcurves,
        ));
        assert!(!b5_supports_follow_edge(
            &supports,
            [[1.0, 0.0, 0.0], [0.0, 0.0, 0.0]],
            [1.5e-3; 2],
            &surfaces,
            &pcurves,
        ));
        let mut reversed_supports = [(10, 20, [1.0, 0.0])];
        orient_b5_supports_to_edge(
            &mut reversed_supports,
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            [1.5e-3; 2],
            &surfaces,
            &pcurves,
        );
        assert_eq!(reversed_supports[0].2, [0.0, 1.0]);
        assert!(b5_supports_follow_edge(
            &reversed_supports,
            [[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            [1.5e-3; 2],
            &surfaces,
            &pcurves,
        ));
        assert!(b5_supports_follow_edge(
            &supports,
            [[0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
            [1.01; 2],
            &surfaces,
            &pcurves,
        ));
    }

    #[test]
    fn descending_nurbs_knots_are_not_promoted_as_curve_caches() {
        let geometry = CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![1.0, 1.0, 0.0, 0.0],
            control_points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)],
            weights: None,
            periodic: false,
        });
        assert!(!curve_cache_has_ordered_knots(&geometry));
    }

    #[test]
    fn exact_revolution_builders_reject_unbounded_subdivision_counts() {
        assert!(rational_arc(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 1.0, 0.0],
            1.0e-300,
            [0.0, 1.0],
        )
        .is_none());
        let profile = cadmpeg_ir::geometry::NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 1.0, 1.0],
            control_points: vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 0.0, 1.0)],
            weights: None,
            periodic: false,
        };
        assert!(revolve_nurbs(
            &profile,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [0.0, 1.0e300],
            [0.0, 1.0],
        )
        .is_none());
        let mut wide_profile = profile;
        wide_profile.control_points = vec![Point3::new(1.0, 0.0, 0.0); 123];
        assert!(revolve_nurbs(
            &wide_profile,
            [0.0; 3],
            [0.0, 0.0, 1.0],
            [0.0, 4096.0 * std::f64::consts::FRAC_PI_2],
            [0.0, 1.0],
        )
        .is_none());
    }

    #[test]
    fn body_kind_requires_unique_complete_loop_ownership() {
        let mut graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
                terminal_control: None,
            }],
            face_records: BTreeMap::new(),
            loops: BTreeMap::from([(
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![4],
                    edges: vec![3],
                    metadata: test_loop_metadata(1),
                    surface: 10,
                },
            )]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices: BTreeMap::from([(3, [0, 1])]),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };

        assert_eq!(
            ownership_plan(&graph)
                .expect("required invariant")
                .body_kind,
            BodyKind::Sheet
        );
        graph.faces[0].loops.push(2);
        assert!(ownership_plan(&graph).is_none());
        graph.faces[0].loops.pop();
        graph.faces.push(B5Face {
            object_id: 5,
            surface: 10,
            loops: vec![2],
            terminal_control: None,
        });
        assert!(ownership_plan(&graph).is_none());
        graph.faces.pop();

        graph.faces.push(B5Face {
            object_id: 5,
            surface: 10,
            loops: vec![6],
            terminal_control: None,
        });
        graph.loops.insert(
            6,
            B5Loop {
                object_id: 6,
                pcurves: vec![8],
                edges: vec![7],
                metadata: test_loop_metadata(1),
                surface: 10,
            },
        );
        graph.edge_vertices.insert(7, [0, 1]);
        let ownership = ownership_plan(&graph).expect("required invariant");
        assert_eq!(ownership.face_components, vec![0, 1]);
        assert_eq!(ownership.components.len(), 2);
        assert_eq!(ownership.body_kind, BodyKind::Sheet);

        graph
            .loops
            .get_mut(&2)
            .expect("required invariant")
            .edges
            .push(3);
        assert_eq!(
            ownership_plan(&graph)
                .expect("required invariant")
                .body_kind,
            BodyKind::General
        );
        graph
            .loops
            .get_mut(&2)
            .expect("required invariant")
            .edges
            .pop();

        graph.loops.get_mut(&6).expect("required invariant").edges[0] = 3;
        let ownership = ownership_plan(&graph).expect("required invariant");
        assert_eq!(ownership.face_components, vec![0, 0]);
        assert_eq!(ownership.components.len(), 1);
        assert_eq!(ownership.body_kind, BodyKind::Solid);

        graph.faces.pop();
        graph.loops.remove(&6);
        graph.edge_vertices.remove(&7);
        graph.edge_vertices.insert(3, [0, 2]);
        assert!(ownership_plan(&graph).is_none());
    }

    #[test]
    fn loop_orientation_reverses_member_order_and_rejects_frustrated_parity() {
        let loop_ = |object_id: u32, edges: Vec<u32>| B5Loop {
            object_id,
            pcurves: vec![0; edges.len()],
            metadata: test_loop_metadata(edges.len()),
            edges,
            surface: 10,
        };
        let mut graph = B5Graph {
            complete: true,
            faces: Vec::new(),
            face_records: BTreeMap::new(),
            loops: BTreeMap::from([(1, loop_(1, vec![3])), (2, loop_(2, vec![4, 5, 3]))]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices: BTreeMap::new(),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        graph
            .loops
            .get_mut(&2)
            .expect("required loop")
            .metadata
            .edge_controls[1][2] = -1;
        let orientation = orient_loop_members(
            &graph,
            BTreeMap::from([(1, vec![false]), (2, vec![false; 3])]),
        )
        .expect("required invariant");
        assert_eq!(orientation[&1].member_order, vec![0]);
        assert_eq!(orientation[&2].member_order, vec![2, 1, 0]);
        assert_eq!(orientation[&1].reversed, vec![false]);
        assert_eq!(orientation[&2].reversed, vec![true; 3]);
        assert_eq!(orientation[&1].pcurve_reversed, vec![false]);
        assert_eq!(orientation[&2].pcurve_reversed, vec![true, false, true]);

        graph.loops = BTreeMap::from([
            (1, loop_(1, vec![1, 3])),
            (2, loop_(2, vec![1, 2])),
            (3, loop_(3, vec![2, 3])),
        ]);
        assert!(orient_loop_members(
            &graph,
            BTreeMap::from([
                (1, vec![false; 2]),
                (2, vec![false; 2]),
                (3, vec![false; 2]),
            ]),
        )
        .is_none());
    }

    #[test]
    fn emitted_carriers_determine_logical_vertex_tolerance() {
        let graph = B5Graph {
            complete: true,
            faces: Vec::new(),
            face_records: BTreeMap::new(),
            loops: BTreeMap::from([(
                1,
                B5Loop {
                    object_id: 1,
                    pcurves: vec![2],
                    edges: vec![3],
                    metadata: test_loop_metadata(1),
                    surface: 4,
                },
            )]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (
                    20,
                    B5ParameterIncidence {
                        object_id: 20,
                        curves: vec![2],
                        parameters: vec![0.25],
                        controls: vec![0],
                    },
                ),
                (
                    21,
                    B5ParameterIncidence {
                        object_id: 21,
                        curves: vec![2],
                        parameters: vec![0.75],
                        controls: vec![0],
                    },
                ),
            ]),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[0.25, 0.0, 1e-4], [0.75, 0.0, 0.0]],
            logical_vertex_refs: vec![10, 11],
            edge_vertices: BTreeMap::from([(3, [0, 1])]),
            edge_parameter_incidences: BTreeMap::from([(3, [20, 21])]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let pcurves = BTreeMap::from([(
            2,
            (
                PcurveGeometry::Nurbs {
                    degree: 1,
                    knots: vec![0.0, 0.0, 1.0, 1.0],
                    control_points: vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
                    weights: None,
                    periodic: false,
                },
                false,
                [0.0, 1.0],
            ),
        )]);
        let surfaces = BTreeMap::from([(
            4,
            SurfacePlan {
                geometry: SurfaceGeometry::Plane {
                    origin: Point3::new(0.0, 0.0, 0.0),
                    normal: Vector3::new(0.0, 0.0, 1.0),
                    u_axis: Vector3::new(1.0, 0.0, 0.0),
                },
                procedure: None,
            },
        )]);
        let supports = HashMap::from([(3, vec![(4, 2, [0.25, 0.75])])]);

        let tolerances = transfer_vertex_tolerances(&graph, &supports, &surfaces, &pcurves);
        assert!((tolerances[&0] - (1e-4 + 1e-9)).abs() < 1e-12);
        assert!(!tolerances.contains_key(&1));
    }

    #[test]
    fn cylinder_pcurve_uses_independent_angular_scale_without_origin_rotation() {
        let surface = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 6.0,
            u_range: [1.0, 1.0 + 6.0 * std::f64::consts::PI],
            v_range: [-1.0, 1.0],
            angular_scale: 3.0,
            chart_origin: 1.0,
        };
        let point = neutral_pcurve_point([3.0 * std::f64::consts::PI, 3.0], &surface);
        assert_eq!(point.u, std::f64::consts::PI);
        assert_eq!(point.v, 3.0);
        let lifted = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            6.0,
            3.0,
            [3.0 * std::f64::consts::PI, 3.0],
        );
        assert!((lifted[0] + 6.0).abs() < 1e-12);
        assert!(lifted[1].abs() < 1e-12);
        assert_eq!(lifted[2], 3.0);
    }

    #[test]
    fn revolution_cache_preserves_native_profile_and_arc_length_chart() {
        let profile = B5Profile::Line {
            point: [2.0, 0.0, 0.0],
            direction: [0.0, 0.0, 1.0],
            parameter_range: [-1.0, 1.0],
        };
        let (surface, plan) = revolution_surface(
            Some(&profile),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            1.0,
            [[-1.0, 1.0], [0.0, std::f64::consts::PI]],
        )
        .expect("exact revolution cache");
        assert_eq!(plan.parameter_interval, [-1.0, 1.0]);
        assert_eq!(plan.angular_interval, [0.0, std::f64::consts::PI]);
        let evaluated = surface_point(
            &SurfaceGeometry::Nurbs(surface),
            0.5,
            std::f64::consts::FRAC_PI_2,
        )
        .expect("surface point");
        assert!(evaluated.x.abs() < 1e-12);
        assert!((evaluated.y - 2.0).abs() < 1e-12);
        assert!((evaluated.z - 0.5).abs() < 1e-12);
        assert!(revolution_surface(
            Some(&profile),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            1.0,
            [[-0.5, 1.0], [0.0, std::f64::consts::PI]],
        )
        .is_none());
    }

    #[test]
    fn revolution_isocurve_keeps_its_native_trim_range() {
        let angular_range = [0.0, std::f64::consts::TAU];
        let graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
                terminal_control: None,
            }],
            face_records: BTreeMap::new(),
            loops: BTreeMap::from([(
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![20],
                    edges: vec![30],
                    metadata: test_loop_metadata(1),
                    surface: 10,
                },
            )]),
            pcurves: BTreeMap::from([(
                20,
                B5Pcurve {
                    object_id: 20,
                    surface: 10,
                    degree: 1,
                    distinct_knots: angular_range.into_iter().collect(),
                    multiplicities: vec![2, 2],
                    control_points: vec![[0.5, angular_range[0]], [0.5, angular_range[1]]],
                    weights: None,
                    parameter_range: None,
                    class_21_suffix_scalar: None,
                    lifted_endpoints: None,
                },
            )]),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::from([(
                10,
                B5Surface::Revolution {
                    profile_curve: 110,
                    axis_origin: [0.0, 0.0, 0.0],
                    reference_x: [1.0, 0.0, 0.0],
                    reference_y: [0.0, 1.0, 0.0],
                    axis_direction: [0.0, 0.0, 1.0],
                    profile_range: [-1.0, 1.0],
                    angular_range,
                    angular_scale: 1.0,
                },
            )]),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::from([
                (
                    40,
                    B5ParameterIncidence {
                        object_id: 40,
                        curves: vec![20],
                        parameters: vec![angular_range[0]],
                        controls: vec![0],
                    },
                ),
                (
                    41,
                    B5ParameterIncidence {
                        object_id: 41,
                        curves: vec![20],
                        parameters: vec![angular_range[1]],
                        controls: vec![0],
                    },
                ),
            ]),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: vec![[2.0, 0.0, 0.5]],
            logical_vertex_refs: vec![50],
            edge_vertices: BTreeMap::from([(30, [0, 0])]),
            edge_parameter_incidences: BTreeMap::from([(30, [40, 41])]),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::from([(
                110,
                B5Profile::Line {
                    point: [2.0, 0.0, 0.0],
                    direction: [0.0, 0.0, 1.0],
                    parameter_range: [-1.0, 1.0],
                },
            )]),
        };
        assert!(matches!(
            resolved_surface_carrier_in_graph(&graph, 10),
            Some(ResolvedPcurveSurface::Geometry(SurfaceGeometry::Nurbs(_)))
        ));
        let plan = build_plan(&graph, &UnknownId("catia:test-payload".to_string()))
            .expect("closed revolution graph");
        let curve = plan.edge_curve_plan.get(&30).expect("revolution isocurve");
        assert_eq!(curve.parameter_range, Some(angular_range));
        assert!(matches!(curve.geometry, CurveGeometry::Nurbs(_)));
    }

    #[test]
    fn affine_and_isoparametric_pcurves_produce_exact_curve_carriers() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 2.0], [3.0, 2.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        let plane = B5Surface::Plane {
            origin: [1.0, 2.0, 3.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let Some(CurveGeometry::Nurbs(curve)) = lifted_curve_geometry(&pcurve, &plane) else {
            panic!("plane lift must be NURBS");
        };
        assert_eq!(curve.control_points[0], Point3::new(1.0, 4.0, 3.0));
        assert_eq!(curve.control_points[1], Point3::new(4.0, 4.0, 3.0));

        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
            u_range: [0.0, 4.0 * std::f64::consts::PI],
            v_range: [-1.0, 1.0],
            angular_scale: 2.0,
            chart_origin: 0.0,
        };
        assert!(matches!(
            lifted_curve_geometry(&pcurve, &cylinder),
            Some(CurveGeometry::Circle { radius: 2.0, .. })
        ));
        let meridian = B5Pcurve {
            control_points: vec![[1.0, -2.0], [1.0, 4.0]],
            ..pcurve
        };
        assert!(matches!(
            lifted_curve_geometry(&meridian, &cylinder),
            Some(CurveGeometry::Line { .. })
        ));
    }

    #[test]
    fn analytic_isocurves_accept_finite_nonzero_scales() {
        let scale = 1e-200;
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [0.5 * scale, 0.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        let cylinder = B5Surface::Cylinder {
            origin: [0.0; 3],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: scale,
            u_range: [0.0, std::f64::consts::TAU * scale],
            v_range: [-scale, scale],
            angular_scale: scale,
            chart_origin: 0.0,
        };
        let geometry = lifted_curve_geometry(&pcurve, &cylinder).expect("cylinder latitude");
        let edge_start = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            scale,
            scale,
            pcurve.control_points[0],
        );
        let edge_end = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            scale,
            scale,
            pcurve.control_points[1],
        );
        assert!(oriented_circle_plan(
            &pcurve,
            &cylinder,
            &geometry,
            [0.0, 1.0],
            edge_start,
            edge_end,
        )
        .is_some());

        let cone = B5Surface::Cone {
            apex: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle: std::f64::consts::FRAC_PI_6,
            pre_angular_range_scalar: 0.0,
            angular_range: [0.0, std::f64::consts::TAU],
            slant_range: [0.0, scale],
            angular_scale: 1.0,
            angular_domain: [0.0, std::f64::consts::TAU],
        };
        let cone_pcurve = B5Pcurve {
            control_points: vec![[0.0, scale], [0.5, scale]],
            ..pcurve.clone()
        };
        assert!(matches!(
            lifted_curve_geometry(&cone_pcurve, &cone),
            Some(CurveGeometry::Circle { radius, .. }) if radius == scale * 0.5
        ));

        let torus = B5Surface::Torus {
            center: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            major_radius: scale,
            minor_radius: scale,
            major_angular_range: [0.0, std::f64::consts::TAU],
            major_angular_domain: [0.0, std::f64::consts::TAU],
            minor_angular_range: [0.0, std::f64::consts::TAU],
            minor_angular_domain: [0.0, std::f64::consts::TAU],
            major_scale: 1.0,
            minor_scale: 1.0,
        };
        let torus_pcurve = B5Pcurve {
            control_points: vec![[0.0, 0.0], [0.5, 0.0]],
            ..pcurve
        };
        assert!(matches!(
            lifted_curve_geometry(&torus_pcurve, &torus),
            Some(CurveGeometry::Circle { radius, .. }) if radius == 2.0 * scale
        ));
    }

    #[test]
    fn affine_plane_lift_preserves_pcurve_weights() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 2,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![3, 3],
            control_points: vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        let plane = B5Surface::Plane {
            origin: [0.0, 0.0, 2.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
            u_range: [-1.0, 1.0],
            v_range: [-1.0, 1.0],
        };
        let Some(CurveGeometry::Nurbs(curve)) = lifted_curve_geometry(&pcurve, &plane) else {
            panic!("expected lifted rational curve");
        };
        assert_eq!(curve.weights, pcurve.weights);
        assert!(curve.control_points.iter().all(|point| point.z == 2.0));
    }

    #[test]
    fn affine_lift_range_orients_and_trims_the_nurbs_carrier() {
        let geometry = CurveGeometry::Nurbs(NurbsCurve {
            degree: 1,
            knots: vec![0.0, 0.0, 10.0, 10.0],
            control_points: vec![Point3::new(0.0, 0.0, 2.0), Point3::new(10.0, 0.0, 2.0)],
            weights: None,
            periodic: false,
        });
        let forward = oriented_nurbs_range(
            geometry.clone(),
            [2.0, 8.0],
            [2.0, 0.0, 2.0],
            [8.0, 0.0, 2.0],
        )
        .expect("forward trimmed range");
        assert_eq!(forward.geometry, geometry);
        assert_eq!(forward.parameter_range, Some([2.0, 8.0]));
        assert_eq!(forward.edge_tolerance, None);

        let reversed = oriented_nurbs_range(
            geometry.clone(),
            [8.0, 2.0],
            [8.0, 0.0, 2.0],
            [2.0, 0.0, 2.0],
        )
        .expect("reversed trimmed range");
        assert_eq!(reversed.parameter_range, Some([2.0, 8.0]));
        let CurveGeometry::Nurbs(reversed) = reversed.geometry else {
            unreachable!();
        };
        assert_eq!(
            reversed.control_points,
            [Point3::new(10.0, 0.0, 2.0), Point3::new(0.0, 0.0, 2.0)]
        );
        assert!(
            oriented_nurbs_range(geometry, [2.0, 8.0], [3.0, 0.0, 2.0], [8.0, 0.0, 2.0]).is_none()
        );

        let tolerant = oriented_nurbs_range(
            CurveGeometry::Nurbs(NurbsCurve {
                degree: 1,
                knots: vec![0.0, 0.0, 10.0, 10.0],
                control_points: vec![Point3::new(0.0, 0.0, 2.0), Point3::new(10.0, 0.0, 2.0)],
                weights: None,
                periodic: false,
            }),
            [2.0, 8.0],
            [2.0, 0.0, 2.0 + 1e-4],
            [8.0, 0.0, 2.0],
        )
        .expect("tolerant trimmed range");
        assert!((tolerant.edge_tolerance.expect("edge tolerance") - (1e-4 + 1e-9)).abs() < 1e-15);
    }

    #[test]
    fn isocurve_range_uses_monotone_varying_surface_coordinate() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 2,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![3, 3],
            control_points: vec![[4.0, 2.0], [4.0, 6.0], [4.0, 10.0]],
            weights: Some(vec![1.0, 2.0, 1.0]),
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        assert_eq!(
            isocurve_endpoint_parameters(&pcurve, [0.25, 0.75]),
            Some([50.0 / 11.0, 82.0 / 11.0])
        );

        let decreasing = B5Pcurve {
            control_points: pcurve.control_points.iter().copied().rev().collect(),
            ..pcurve.clone()
        };
        assert_eq!(
            isocurve_endpoint_parameters(&decreasing, [0.25, 0.75]),
            Some([82.0 / 11.0, 50.0 / 11.0])
        );

        let turnback = B5Pcurve {
            control_points: vec![[4.0, 2.0], [4.0, 10.0], [4.0, 6.0]],
            ..pcurve.clone()
        };
        assert!(isocurve_endpoint_parameters(&turnback, [0.0, 1.0]).is_none());

        let nonpositive_weight = B5Pcurve {
            weights: Some(vec![1.0, 0.0, 1.0]),
            ..pcurve
        };
        assert!(isocurve_endpoint_parameters(&nonpositive_weight, [0.0, 1.0]).is_none());
    }

    #[test]
    fn analytic_line_range_uses_oriented_signed_distance() {
        let line = CurveGeometry::Line {
            origin: Point3::new(1.0, 2.0, 3.0),
            direction: Vector3::new(0.0, 0.0, 2.0),
        };
        let forward = oriented_line_plan(&line, [1.0, 2.0, 5.0], [1.0, 2.0, 9.0])
            .expect("forward line range");
        assert_eq!(forward.parameter_range, Some([2.0, 6.0]));
        assert!(matches!(
            forward.geometry,
            CurveGeometry::Line { direction, .. }
                if direction == Vector3::new(0.0, 0.0, 1.0)
        ));

        let reversed = oriented_line_plan(&line, [1.0, 2.0, 9.0], [1.0, 2.0, 5.0])
            .expect("reversed line range");
        assert_eq!(reversed.parameter_range, Some([-6.0, -2.0]));
        assert!(matches!(
            reversed.geometry,
            CurveGeometry::Line { direction, .. }
                if direction == Vector3::new(0.0, 0.0, -1.0)
        ));
        let tolerant = oriented_line_plan(&line, [1.001, 2.0, 5.0], [1.0, 2.0, 9.0])
            .expect("tolerant line endpoints");
        assert!(tolerant.edge_tolerance.is_some_and(|value| value > 0.001));
        assert_eq!(tolerant.cache_fit_tolerance, None);
        assert!(oriented_line_plan(&line, [1.01, 2.0, 5.0], [1.0, 2.0, 9.0]).is_none());
        assert!(oriented_line_plan(&line, [1.0, 2.0, 5.0], [1.0, 2.0, 5.0]).is_none());

        let tiny_direction = CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1e-200, 0.0, 0.0),
        };
        let tiny = oriented_line_plan(&tiny_direction, [2.0, 0.0, 0.0], [3.0, 0.0, 0.0])
            .expect("finite nonzero line direction");
        assert!(matches!(
            tiny.geometry,
            CurveGeometry::Line { direction, .. }
                if direction == Vector3::new(1.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn isoparametric_circle_range_preserves_winding_and_seams() {
        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
            u_range: [0.0, 4.0 * std::f64::consts::PI],
            v_range: [-1.0, 1.0],
            angular_scale: 2.0,
            chart_origin: 0.0,
        };
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[11.0, 3.0], [13.0, 3.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        let geometry = lifted_curve_geometry(&pcurve, &cylinder).expect("cylinder latitude");
        let edge_start = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0,
            2.0,
            pcurve.control_points[0],
        );
        let edge_end = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0,
            2.0,
            pcurve.control_points[1],
        );
        let forward = oriented_circle_plan(
            &pcurve,
            &cylinder,
            &geometry,
            [0.0, 1.0],
            edge_start,
            edge_end,
        )
        .expect("seam-crossing circle range");
        assert_eq!(forward.parameter_range, Some([5.5, 6.5]));

        let tiny_sweep = 1e-14;
        let tiny_pcurve = B5Pcurve {
            control_points: vec![[0.0, 3.0], [2.0 * tiny_sweep, 3.0]],
            ..pcurve.clone()
        };
        let tiny_geometry =
            lifted_curve_geometry(&tiny_pcurve, &cylinder).expect("tiny cylinder latitude");
        let tiny_end = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0,
            2.0,
            tiny_pcurve.control_points[1],
        );
        let tiny = oriented_circle_plan(
            &tiny_pcurve,
            &cylinder,
            &tiny_geometry,
            [0.0, 1.0],
            [2.0, 0.0, 3.0],
            tiny_end,
        )
        .expect("tiny circle sweep");
        assert_eq!(tiny.parameter_range, Some([0.0, tiny_sweep]));

        let reversed_pcurve = B5Pcurve {
            control_points: pcurve.control_points.iter().copied().rev().collect(),
            ..pcurve.clone()
        };
        let reversed = oriented_circle_plan(
            &reversed_pcurve,
            &cylinder,
            &geometry,
            [0.0, 1.0],
            edge_end,
            edge_start,
        )
        .expect("reversed circle range");
        let [start, end] = reversed.parameter_range.expect("canonical range");
        assert!(start >= 0.0 && end > start && end - start == 1.0);
        assert!(matches!(
            reversed.geometry,
            CurveGeometry::Circle { axis, .. } if axis == Vector3::new(0.0, 0.0, -1.0)
        ));

        let turnback = B5Pcurve {
            degree: 2,
            multiplicities: vec![3, 3],
            control_points: vec![[0.0, 3.0], [4.0, 3.0], [2.0, 3.0]],
            ..pcurve
        };
        let turnback_geometry =
            lifted_curve_geometry(&turnback, &cylinder).expect("turnback latitude locus");
        let turnback_end = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0,
            2.0,
            [2.0, 3.0],
        );
        assert!(oriented_circle_plan(
            &turnback,
            &cylinder,
            &turnback_geometry,
            [0.0, 1.0],
            [2.0, 0.0, 3.0],
            turnback_end,
        )
        .is_none());

        let half_angle = std::f64::consts::FRAC_PI_6;
        let cone = B5Surface::Cone {
            apex: [0.0; 3],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle,
            pre_angular_range_scalar: 0.0,
            angular_range: [0.0, std::f64::consts::TAU],
            slant_range: [-4.0, 0.0],
            angular_scale: 2.0,
            angular_domain: [0.0, std::f64::consts::TAU],
        };
        let cone_pcurve = B5Pcurve {
            control_points: vec![[0.0, -4.0], [2.0, -4.0]],
            ..reversed_pcurve
        };
        let cone_geometry =
            lifted_curve_geometry(&cone_pcurve, &cone).expect("signed cone latitude");
        let cone_point = |angle: f64| {
            [
                -4.0 * half_angle.sin() * angle.cos(),
                -4.0 * half_angle.sin() * angle.sin(),
                -4.0 * half_angle.cos(),
            ]
        };
        let signed = oriented_circle_plan(
            &cone_pcurve,
            &cone,
            &cone_geometry,
            [0.0, 1.0],
            cone_point(0.0),
            cone_point(1.0),
        )
        .expect("normalized signed-radius circle");
        assert!(matches!(
            signed.geometry,
            CurveGeometry::Circle { radius, ref_direction, .. }
                if radius == 2.0 && ref_direction == Vector3::new(-1.0, 0.0, 0.0)
        ));
    }

    #[test]
    fn edge_curve_plans_merge_proofs_and_discard_conflicting_carriers() {
        let geometry = CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        };
        let mut plans = HashMap::new();
        let mut conflicts = HashSet::new();
        merge_curve_plan(
            &mut plans,
            &mut conflicts,
            4,
            CurvePlan {
                geometry: geometry.clone(),
                parameter_range: None,
                edge_tolerance: None,
                cache_fit_tolerance: None,
            },
        );
        merge_curve_plan(
            &mut plans,
            &mut conflicts,
            4,
            CurvePlan {
                geometry,
                parameter_range: Some([2.0, 8.0]),
                edge_tolerance: None,
                cache_fit_tolerance: None,
            },
        );
        assert_eq!(plans[&4].parameter_range, Some([2.0, 8.0]));

        let conflicting = CurvePlan {
            geometry: CurveGeometry::Line {
                origin: Point3::new(0.0, 1.0, 0.0),
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
            parameter_range: Some([2.0, 8.0]),
            edge_tolerance: None,
            cache_fit_tolerance: None,
        };
        merge_curve_plan(&mut plans, &mut conflicts, 4, conflicting.clone());
        assert!(!plans.contains_key(&4));
        assert!(conflicts.contains(&4));
        merge_curve_plan(&mut plans, &mut conflicts, 4, conflicting);
        assert!(!plans.contains_key(&4));
    }

    #[test]
    fn cone_chart_normalizes_arc_length_and_slant_coordinates() {
        let half_angle = std::f64::consts::FRAC_PI_6;
        let cone = B5Surface::Cone {
            apex: [0.0, 0.0, 0.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            half_angle,
            pre_angular_range_scalar: 0.0,
            angular_range: [0.0, std::f64::consts::TAU],
            slant_range: [2.0, 8.0],
            angular_scale: 3.0,
            angular_domain: [0.0, std::f64::consts::TAU],
        };
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 3.0 * std::f64::consts::PI],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 4.0], [3.0 * std::f64::consts::PI, 4.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        assert_eq!(
            pcurve
                .control_points
                .iter()
                .map(|point| neutral_pcurve_point(*point, &cone))
                .collect::<Vec<_>>(),
            [
                Point2::new(0.0, 2.0 * half_angle.cos()),
                Point2::new(std::f64::consts::PI, 2.0 * half_angle.cos()),
            ]
        );
        let mut opposite_handed = cone.clone();
        let B5Surface::Cone { axis, .. } = &mut opposite_handed else {
            unreachable!();
        };
        *axis = [0.0, 0.0, -1.0];
        assert_eq!(
            neutral_pcurve_point([3.0 * std::f64::consts::PI, 4.0], &opposite_handed),
            Point2::new(-std::f64::consts::PI, 2.0 * half_angle.cos())
        );
        let Some(CurveGeometry::Circle {
            center,
            radius,
            axis,
            ..
        }) = lifted_curve_geometry(&pcurve, &cone)
        else {
            panic!("expected cone latitude circle");
        };
        assert_eq!(center, Point3::new(0.0, 0.0, 4.0 * half_angle.cos()));
        assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
        assert!((radius - 2.0).abs() < 1e-12);
    }

    #[test]
    fn sphere_class_1d_fields_lift_to_the_exact_great_circle_plane() {
        let sphere = B5Surface::Sphere {
            center: [1.0, 2.0, 3.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 5.0,
            azimuth_range: [0.0, std::f64::consts::TAU],
            latitude_range: [-1.0, 1.0],
            construction_radius: 8.0,
            chart_origin: 0.0,
        };
        let pcurve = B5SphereGreatCirclePcurve {
            chart_bounds: [[0.0, 8.0], [0.0, std::f64::consts::TAU * 8.0]],
            chart_shift: 0.0,
            chart_scale: 8.0,
            slope: -1.0,
            phase: -std::f64::consts::FRAC_PI_2,
        };
        let Some(CurveGeometry::Circle {
            center,
            axis,
            ref_direction,
            radius,
        }) = sphere_great_circle_geometry(&pcurve, &sphere)
        else {
            panic!("expected great circle");
        };
        assert_eq!(center, Point3::new(1.0, 2.0, 3.0));
        assert!((radius - 5.0).abs() < 1e-12);
        assert!((axis.x * axis.x + axis.y * axis.y + axis.z * axis.z - 1.0).abs() < 1e-12);
        assert!(
            (axis.x * ref_direction.x + axis.y * ref_direction.y + axis.z * ref_direction.z).abs()
                < 1e-12
        );
        assert!((axis.y + std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);
        assert!((axis.z - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-12);

        let (geometry, range) =
            sphere_great_circle_pcurve(&pcurve).expect("exact parameter-space curve");
        assert_eq!(range, [0.0, 8.0]);
        let uv = cadmpeg_ir::eval::pcurve_uv(&geometry, 8.0).expect("chart endpoint");
        assert_eq!(uv.u, 1.0);
        assert!((uv.v - (-(1.0 + std::f64::consts::FRAC_PI_2).cos()).atan()).abs() < 1e-12);

        let tiny = 1e-200;
        let mut tiny_sphere = sphere;
        let B5Surface::Sphere {
            construction_radius,
            radius,
            ..
        } = &mut tiny_sphere
        else {
            unreachable!()
        };
        *construction_radius = tiny;
        *radius = tiny;
        let tiny_pcurve = B5SphereGreatCirclePcurve {
            chart_bounds: [[0.0, tiny], [0.0, std::f64::consts::TAU * tiny]],
            chart_shift: 0.0,
            chart_scale: tiny,
            slope: -1.0,
            phase: 0.0,
        };
        assert!(sphere_great_circle_geometry(&tiny_pcurve, &tiny_sphere).is_some());
        let (geometry, range) =
            sphere_great_circle_pcurve(&tiny_pcurve).expect("tiny parameter-space curve");
        assert_eq!(range, [0.0, tiny]);
        let uv = cadmpeg_ir::eval::pcurve_uv(&geometry, tiny).expect("tiny chart endpoint");
        assert_eq!(uv.u, 1.0);
    }

    #[test]
    fn owned_sphere_class_1d_pcurve_enters_the_transfer_plan() {
        let chart_scale = 8.0;
        let parameter_range = [0.0, 4.0 * std::f64::consts::PI];
        let graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 2,
                loops: vec![3],
                terminal_control: None,
            }],
            face_records: BTreeMap::new(),
            loops: BTreeMap::from([(
                3,
                B5Loop {
                    object_id: 3,
                    pcurves: vec![4, 4, 4],
                    edges: vec![5, 6, 7],
                    metadata: test_loop_metadata(3),
                    surface: 2,
                },
            )]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::from([(
                4,
                B5OpaquePcurve {
                    object_id: 4,
                    surface: 2,
                    class: 0x1d,
                    payload: Vec::new(),
                    sphere_great_circle: Some(B5SphereGreatCirclePcurve {
                        chart_bounds: [parameter_range, [0.0, std::f64::consts::TAU * chart_scale]],
                        chart_shift: 0.0,
                        chart_scale,
                        slope: 0.0,
                        phase: 0.0,
                    }),
                },
            )]),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::from([(
                2,
                B5Surface::Sphere {
                    center: [0.0, 0.0, 0.0],
                    direction_x: [1.0, 0.0, 0.0],
                    direction_y: [0.0, 1.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    radius: 5.0,
                    azimuth_range: [0.0, std::f64::consts::TAU],
                    latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
                    construction_radius: chart_scale,
                    chart_origin: 0.0,
                },
            )]),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: vec![[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [-5.0, 0.0, 0.0]],
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices: BTreeMap::from([(5, [0, 1]), (6, [1, 2]), (7, [2, 0])]),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let payload = UnknownId("catia:test-payload".to_string());

        assert!(ownership_plan(&graph).is_some());
        assert!(loop_chain_closes(&graph.loops[&3], &graph.edge_vertices));
        let senses = graph.loops[&3].edge_senses();
        assert!(orient_loop_members(&graph, BTreeMap::from([(3, senses)])).is_some());
        let plan = build_plan(&graph, &payload).expect("complete owned graph");

        assert_eq!(
            plan.pcurve_plan.get(&4),
            Some(&(
                PcurveGeometry::SphericalGreatCircle {
                    azimuth_origin: 0.0,
                    azimuth_rate: chart_scale.recip(),
                    plane_phase: 0.0,
                    plane_slope: 0.0,
                },
                false,
                parameter_range,
            ))
        );
        assert_eq!(
            plan.edge_support_plan.get(&5),
            Some(&vec![(2, 4, parameter_range)])
        );
        assert!(plan.exact_support_edges.contains(&5));

        let mut ir = CadIr::empty(Units::default());
        assert!(transfer(
            &mut ir,
            &mut AnnotationBuilder::new(),
            graph,
            &payload,
        ));
        assert_eq!(ir.model.pcurves.len(), 1);
        assert!(matches!(
            ir.model.pcurves[0].geometry,
            PcurveGeometry::SphericalGreatCircle { .. }
        ));
    }

    /// One closed spherical component of a synthetic B5 graph. `face`, `loop_`,
    /// `pcurve`, and `surface` are persistent object ids; `edges` names the three
    /// edge object ids and `vertices` the three vertex-point rows.
    struct SyntheticSphericalComponent {
        face: u32,
        loop_: u32,
        pcurve: u32,
        surface: u32,
        edges: [u32; 3],
        vertices: [usize; 3],
        center: [f64; 3],
    }

    /// Build a B5 graph of independent closed spherical components. Each
    /// component contributes one face carrying one three-member loop over a
    /// single class-`1d` great-circle pcurve, as in
    /// [`owned_sphere_class_1d_pcurve_enters_the_transfer_plan`].
    fn synthetic_spherical_graph(components: &[SyntheticSphericalComponent]) -> B5Graph {
        let chart_scale = 8.0;
        let parameter_range = [0.0, 4.0 * std::f64::consts::PI];
        let radius = 5.0;
        let mut graph = B5Graph {
            complete: true,
            faces: Vec::new(),
            face_records: BTreeMap::new(),
            loops: BTreeMap::new(),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            surface_aliases: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            edges: BTreeMap::new(),
            vertex_incidence_links: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices: BTreeMap::new(),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        for component in components {
            graph.faces.push(B5Face {
                object_id: component.face,
                surface: component.surface,
                loops: vec![component.loop_],
                terminal_control: None,
            });
            graph.loops.insert(
                component.loop_,
                B5Loop {
                    object_id: component.loop_,
                    pcurves: vec![component.pcurve; 3],
                    edges: component.edges.to_vec(),
                    metadata: test_loop_metadata(3),
                    surface: component.surface,
                },
            );
            graph.opaque_pcurves.insert(
                component.pcurve,
                B5OpaquePcurve {
                    object_id: component.pcurve,
                    surface: component.surface,
                    class: 0x1d,
                    payload: Vec::new(),
                    sphere_great_circle: Some(B5SphereGreatCirclePcurve {
                        chart_bounds: [parameter_range, [0.0, std::f64::consts::TAU * chart_scale]],
                        chart_shift: 0.0,
                        chart_scale,
                        slope: 0.0,
                        phase: 0.0,
                    }),
                },
            );
            graph.surfaces.insert(
                component.surface,
                B5Surface::Sphere {
                    center: component.center,
                    direction_x: [1.0, 0.0, 0.0],
                    direction_y: [0.0, 1.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    radius,
                    azimuth_range: [0.0, std::f64::consts::TAU],
                    latitude_range: [-std::f64::consts::FRAC_PI_2, std::f64::consts::FRAC_PI_2],
                    construction_radius: chart_scale,
                    chart_origin: 0.0,
                },
            );
            let points = [
                [
                    component.center[0] + radius,
                    component.center[1],
                    component.center[2],
                ],
                [
                    component.center[0],
                    component.center[1] + radius,
                    component.center[2],
                ],
                [
                    component.center[0] - radius,
                    component.center[1],
                    component.center[2],
                ],
            ];
            for (row, point) in component.vertices.into_iter().zip(points) {
                assert_eq!(row, graph.vertex_points.len(), "contiguous vertex rows");
                graph.vertex_points.push(point);
            }
            for (position, edge) in component.edges.into_iter().enumerate() {
                graph.edge_vertices.insert(
                    edge,
                    [
                        component.vertices[position],
                        component.vertices[(position + 1) % 3],
                    ],
                );
            }
        }
        graph
    }

    /// B5 object ids carry an unpadded decimal key, so a face pair such as
    /// `#9`/`#10` reaches the neutral model in ascending native order while
    /// sorting the other way. The route must still produce an admissible model:
    /// every cross-reference is an id string, so canonical arena order is a
    /// property the pipeline restores rather than one the emit passes owe.
    ///
    /// Two ownership components put several arenas out of sorted order at once,
    /// which one face cannot do. The container-level counterpart is
    /// `tests::decode_float_packed_stream_transfers_topology_under_decimal_object_ids`.
    #[test]
    fn decimal_object_id_keys_transfer_to_an_admissible_model() {
        let graph = synthetic_spherical_graph(&[
            SyntheticSphericalComponent {
                face: 9,
                loop_: 29,
                pcurve: 39,
                surface: 2,
                edges: [49, 50, 51],
                vertices: [0, 1, 2],
                center: [0.0, 0.0, 0.0],
            },
            SyntheticSphericalComponent {
                face: 10,
                loop_: 200,
                pcurve: 300,
                surface: 12,
                edges: [400, 401, 402],
                vertices: [3, 4, 5],
                center: [100.0, 0.0, 0.0],
            },
        ]);

        let mut ir = CadIr::empty(Units::default());
        assert!(transfer(
            &mut ir,
            &mut AnnotationBuilder::new(),
            graph,
            &UnknownId("catia:payload:unknown#test".to_string()),
        ));

        // Native traversal order, which the arena-order check reads as unsorted.
        assert_eq!(
            ir.model
                .faces
                .iter()
                .map(|face| face.id.0.as_str())
                .collect::<Vec<_>>(),
            ["catia:b5:face#9", "catia:b5:face#10"]
        );
        assert_eq!(ir.model.loops.len(), 2);
        assert_eq!(ir.model.shells.len(), 2);
        assert_eq!(ir.model.regions.len(), 2);
        assert_eq!(ir.model.edges.len(), 6);
        assert_eq!(ir.model.coedges.len(), 6);
        assert_eq!(ir.model.vertices.len(), 6);
        assert_eq!(ir.model.pcurves.len(), 2);
        let unsorted_arenas = cadmpeg_ir::validate::validate(&ir, Vec::new())
            .findings
            .iter()
            .filter(|finding| finding.check == cadmpeg_ir::report::Check::ArenaOrder)
            .count();
        assert!(
            unsorted_arenas >= 6,
            "one component cannot unsort this many arenas: {unsorted_arenas}"
        );

        assert!(crate::assemble::neutral_model_is_admissible(&mut ir, &[]));
        assert_eq!(
            ir.model
                .faces
                .iter()
                .map(|face| face.id.0.as_str())
                .collect::<Vec<_>>(),
            ["catia:b5:face#10", "catia:b5:face#9"]
        );
    }

    #[test]
    fn torus_chart_lifts_meridians_and_latitudes_exactly() {
        let torus = B5Surface::Torus {
            center: [0.0, 0.0, 0.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            major_radius: 5.0,
            minor_radius: 2.0,
            major_angular_range: [0.0, std::f64::consts::TAU],
            major_angular_domain: [0.0, std::f64::consts::TAU],
            minor_angular_range: [0.0, std::f64::consts::TAU],
            minor_angular_domain: [0.0, std::f64::consts::TAU],
            major_scale: 5.0,
            minor_scale: 2.0,
        };
        let base = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 0.0], [0.0, 4.0 * std::f64::consts::PI]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        assert_eq!(
            neutral_pcurve_point([5.0 * std::f64::consts::PI, 2.0], &torus),
            Point2::new(std::f64::consts::PI, 1.0)
        );
        let Some(CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        }) = lifted_curve_geometry(&base, &torus)
        else {
            panic!("expected meridian circle");
        };
        assert_eq!(center, Point3::new(5.0, 0.0, 0.0));
        assert_eq!(axis, Vector3::new(0.0, -1.0, 0.0));
        assert_eq!(radius, 2.0);

        let latitude = B5Pcurve {
            control_points: vec![[0.0, 0.0], [10.0 * std::f64::consts::PI, 0.0]],
            ..base
        };
        let Some(CurveGeometry::Circle {
            center,
            axis,
            radius,
            ..
        }) = lifted_curve_geometry(&latitude, &torus)
        else {
            panic!("expected latitude circle");
        };
        assert_eq!(center, Point3::new(0.0, 0.0, 0.0));
        assert_eq!(axis, Vector3::new(0.0, 0.0, 1.0));
        assert_eq!(radius, 7.0);
    }

    #[test]
    fn tensor_surface_contraction_preserves_exact_isocurve() {
        let surface = cadmpeg_ir::geometry::NurbsSurface {
            u_degree: 1,
            v_degree: 1,
            u_knots: vec![0.0, 0.0, 1.0, 1.0],
            v_knots: vec![0.0, 0.0, 1.0, 1.0],
            u_count: 2,
            v_count: 2,
            control_points: vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 1.0, 0.0),
                Point3::new(2.0, 0.0, 0.0),
                Point3::new(2.0, 1.0, 2.0),
            ],
            weights: None,
            u_periodic: false,
            v_periodic: false,
        };
        let curve = crate::nurbs::nurbs_surface_isocurve(&surface, 0.25, true).expect("u isocurve");
        assert_eq!(curve.degree, 1);
        assert_eq!(curve.knots, surface.v_knots);
        assert_eq!(curve.control_points[0], Point3::new(0.5, 0.0, 0.0));
        assert_eq!(curve.control_points[1], Point3::new(0.5, 1.0, 0.5));
    }

    #[test]
    fn affine_cylinder_pcurve_preserves_exact_helix_construction() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 3.0], [4.0, 7.0]],
            weights: None,
            parameter_range: None,
            class_21_suffix_scalar: None,
            lifted_endpoints: None,
        };
        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
            u_range: [0.0, 4.0 * std::f64::consts::PI],
            v_range: [-1.0, 1.0],
            angular_scale: 2.0,
            chart_origin: 0.0,
        };
        let end = [2.0 * 2.0_f64.cos(), 2.0 * 2.0_f64.sin(), 7.0];
        let Some(plan) = cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], [2.0, 0.0, 3.0], end)
        else {
            panic!("degree-one cylinder helix");
        };
        let ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            pitch,
            apex_factor,
            ..
        } = &plan.definition
        else {
            unreachable!();
        };
        assert_eq!(*angle_range, [0.0, 2.0]);
        assert_eq!(*center, Point3::new(0.0, 0.0, 3.0));
        assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1e-12);
        assert_eq!(*apex_factor, 0.0);
        assert_eq!(plan.parameter_range, [0.0, 2.0]);
        assert!(plan.fit_tolerance <= 1e-4);
        assert_eq!(
            plan.cache.control_points.first(),
            Some(&Point3::new(2.0, 0.0, 3.0))
        );

        let reversed = cylinder_helix(&pcurve, &cylinder, [0.0, 1.0], end, [2.0, 0.0, 3.0])
            .expect("reversed physical edge helix");
        let ProceduralCurveDefinition::Helix { center, pitch, .. } = reversed.definition else {
            unreachable!();
        };
        assert_eq!(center, Point3::new(0.0, 0.0, 7.0));
        assert!((pitch.z + 4.0 * std::f64::consts::PI).abs() < 1e-12);

        let trimmed_start = [2.0 * 0.5_f64.cos(), 2.0 * 0.5_f64.sin(), 4.0];
        let trimmed_end = [2.0 * 1.5_f64.cos(), 2.0 * 1.5_f64.sin(), 6.0];
        let trimmed = cylinder_helix(&pcurve, &cylinder, [0.25, 0.75], trimmed_start, trimmed_end)
            .expect("trimmed physical edge helix");
        let ProceduralCurveDefinition::Helix {
            angle_range,
            center,
            pitch,
            ..
        } = trimmed.definition
        else {
            unreachable!();
        };
        assert_eq!(angle_range, [0.0, 1.0]);
        assert_eq!(center.z, 4.0);
        assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1e-12);

        let tiny = 1e-14;
        let tiny_pcurve = B5Pcurve {
            control_points: vec![[0.0, 0.0], [2.0 * tiny, 2.0 * tiny]],
            ..pcurve
        };
        let tiny_end = [2.0 * tiny.cos(), 2.0 * tiny.sin(), 2.0 * tiny];
        let tiny_plan = cylinder_helix(
            &tiny_pcurve,
            &cylinder,
            [0.0, 1.0],
            [2.0, 0.0, 0.0],
            tiny_end,
        )
        .expect("tiny helix sweep");
        let ProceduralCurveDefinition::Helix {
            angle_range, pitch, ..
        } = tiny_plan.definition
        else {
            unreachable!();
        };
        assert_eq!(angle_range, [0.0, tiny]);
        assert!((pitch.z - 4.0 * std::f64::consts::PI).abs() < 1e-12);
    }
}
