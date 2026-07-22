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

use super::graph::{loop_chain_senses, B5Graph, B5Surface};

mod edges;
mod faces;
mod pcurves;
mod surfaces;
mod vertices;

use edges::{
    b5_supports_agree, b5_supports_follow_curve, b5_supports_follow_edge, b5_vertex_point,
    bounded_occurrence_range, curve_cache_has_ordered_knots, edge_pcurve_parameters,
    merge_curve_plan, orient_b5_supports_to_edge,
};
use faces::{orient_loop_members, ownership_plan};
use pcurves::{
    cylinder_helix, isocurve_endpoint_parameters, lifted_curve_geometry, neutral_pcurve_point,
    nurbs_isocurve, oriented_circle_plan, oriented_line_plan, oriented_nurbs_range,
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

enum SurfaceProcedure {
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
                && loop_chain_senses(loop_, &graph.edge_vertices).is_some()
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
    let pcurve_ids = pcurves::emit_pcurves(ir, annotations, graph, &plan);
    let edge_id_map = edges::emit_edges(ir, annotations, graph, payload, &mut plan, &surface_ids);
    faces::emit_faces(
        ir,
        annotations,
        graph,
        &plan,
        &surface_ids,
        &pcurve_ids,
        &edge_id_map,
    );
    true
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

    let mut referenced_surfaces: HashSet<u32> =
        graph.faces.iter().map(|face| face.surface).collect();
    let mut pending_surfaces: Vec<u32> = referenced_surfaces.iter().copied().collect();
    while let Some(surface_id) = pending_surfaces.pop() {
        let Some(offset) = graph.offset_surfaces.get(&surface_id) else {
            continue;
        };
        if referenced_surfaces.insert(offset.source_surface) {
            pending_surfaces.push(offset.source_surface);
        }
    }
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
        let senses = loop_chain_senses(loop_, &graph.edge_vertices)?;
        loop_senses.insert(loop_.object_id, senses);
        for (&pcurve_id, &edge_id) in loop_.pcurves.iter().zip(&loop_.edges) {
            let Some(pcurve) = graph.pcurves.get(&pcurve_id) else {
                if graph
                    .opaque_pcurves
                    .get(&pcurve_id)
                    .is_some_and(|pcurve| pcurve.surface == loop_.surface)
                    || graph.implicit_pcurves.get(&pcurve_id) == Some(&loop_.surface)
                {
                    edge_ids.insert(edge_id);
                    continue;
                }
                return None;
            };
            if pcurve.surface != loop_.surface || !graph.edge_vertices.contains_key(&edge_id) {
                return None;
            }
            let knots = expand_knots(&pcurve.distinct_knots, &pcurve.multiplicities)?;
            let degree = usize::try_from(pcurve.degree).ok()?;
            let parameter_range = knots
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
                } else if matches!(surface, B5Surface::Nurbs(_)) {
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
        SurfaceProcedure::Revolution(_) => None,
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
    /// Fit tolerance of the retained sampled directrix cache.
    pub(crate) cache_fit_tolerance: f64,
    /// Unit world-space extrusion direction.
    pub(crate) direction: Vector3,
    /// Ordered exact support sides.
    pub(crate) supports: [ResolvedExtrusionSupport; 2],
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
}

pub(crate) fn resolved_extrusion_surface(
    graph: &B5Graph,
    surface_id: u32,
) -> Option<ResolvedExtrusionSurface> {
    let extrusion = graph.extrusion_surfaces.get(&surface_id)?;
    let supports: [ResolvedExtrusionSupport; 2] = extrusion
        .directrix
        .supports
        .each_ref()
        .map(
            |&(surface_object_id, pcurve_object_id, pcurve_parameter_range)| {
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
                Some(ResolvedExtrusionSupport {
                    surface_object_id,
                    surface,
                    pcurve: PcurveGeometry::Nurbs {
                        degree: pcurve.degree,
                        knots,
                        control_points: pcurve
                            .control_points
                            .iter()
                            .map(|point| neutral_pcurve_point(*point, source_surface))
                            .collect(),
                        weights: pcurve.weights.clone(),
                        periodic: false,
                    },
                    pcurve_parameter_range,
                })
            },
        )
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()?;
    (supports[0].surface_object_id != supports[1].surface_object_id).then_some(())?;
    Some(ResolvedExtrusionSurface {
        surface_object_id: extrusion.object_id,
        directrix_object_id: extrusion.directrix.object_id,
        directrix_parameter_range: extrusion.directrix.parameter_range,
        cache_fit_tolerance: extrusion.directrix.cache_fit_tolerance,
        direction: vector(extrusion.direction),
        supports,
    })
}

pub(crate) fn resolved_offset_surface(
    graph: &B5Graph,
    surface_id: u32,
) -> Option<ResolvedOffsetSurface> {
    let offset = graph.offset_surfaces.get(&surface_id)?;
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

// `unit` normalizes by per-component division, a bit-level-distinct form from
// the parse graph's reciprocal-multiply (`graph::unit`). The two must NOT be
// unified: the affected profiles depend on the exact rounding of each form.
fn unit(value: [f64; 3]) -> Option<[f64; 3]> {
    let length = length(value);
    (length > f64::EPSILON).then(|| [value[0] / length, value[1] / length, value[2] / length])
}
#[cfg(test)]
mod tests {
    use super::super::graph::{
        loop_chain_senses, B5Face, B5Graph, B5Loop, B5ParameterIncidence, B5Pcurve, B5Profile,
        B5Surface,
    };
    use super::edges::{
        b5_edge_support_definition, b5_supports_follow_edge, bounded_occurrence_range,
        curve_cache_has_ordered_knots, edge_pcurve_parameters, merge_curve_plan, ordered_subrange,
        orient_b5_supports_to_edge,
    };
    use super::faces::{orient_loop_members, ownership_plan};
    use super::pcurves::{
        cylinder_helix, cylinder_point, isocurve_endpoint_parameters, lifted_curve_geometry,
        neutral_pcurve_point, oriented_circle_plan, oriented_line_plan, oriented_nurbs_range,
    };
    use super::surfaces::{rational_arc, revolution_surface, revolve_nurbs};
    use super::vertices::transfer_vertex_tolerances;
    use super::{transfer, CurvePlan, SurfacePlan};
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
    }

    #[test]
    fn closed_one_edge_loop_uses_native_edge_direction() {
        let loop_ = B5Loop {
            object_id: 1,
            pcurves: vec![2],
            edges: vec![3],
            surface: 4,
        };

        assert_eq!(
            loop_chain_senses(&loop_, &BTreeMap::from([(3, [0, 0])])),
            Some(vec![false])
        );
        assert_eq!(
            loop_chain_senses(&loop_, &BTreeMap::from([(3, [0, 1])])),
            None
        );
    }

    #[test]
    fn edge_parameters_follow_ordered_edge_refs_for_a_closed_vertex() {
        let mut graph = B5Graph {
            complete: false,
            faces: Vec::new(),
            loops: BTreeMap::new(),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
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

    #[test]
    fn repeated_source_pcurve_has_independently_trimmed_occurrence_carriers() {
        let graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
            }],
            loops: BTreeMap::from([(
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![20, 20, 20],
                    edges: vec![30, 31, 32],
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
                },
            )]),
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
    }

    #[test]
    fn body_kind_requires_unique_complete_loop_ownership() {
        let mut graph = B5Graph {
            complete: true,
            faces: vec![B5Face {
                object_id: 1,
                surface: 10,
                loops: vec![2],
            }],
            loops: BTreeMap::from([(
                2,
                B5Loop {
                    object_id: 2,
                    pcurves: vec![4],
                    edges: vec![3],
                    surface: 10,
                },
            )]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            vertex_points: vec![[0.0; 3], [1.0, 0.0, 0.0]],
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices: BTreeMap::from([(3, [0, 1])]),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };

        assert_eq!(ownership_plan(&graph).unwrap().body_kind, BodyKind::Sheet);
        graph.faces[0].loops.push(2);
        assert!(ownership_plan(&graph).is_none());
        graph.faces[0].loops.pop();
        graph.faces.push(B5Face {
            object_id: 5,
            surface: 10,
            loops: vec![2],
        });
        assert!(ownership_plan(&graph).is_none());
        graph.faces.pop();

        graph.faces.push(B5Face {
            object_id: 5,
            surface: 10,
            loops: vec![6],
        });
        graph.loops.insert(
            6,
            B5Loop {
                object_id: 6,
                pcurves: vec![8],
                edges: vec![7],
                surface: 10,
            },
        );
        graph.edge_vertices.insert(7, [0, 1]);
        let ownership = ownership_plan(&graph).unwrap();
        assert_eq!(ownership.face_components, vec![0, 1]);
        assert_eq!(ownership.components.len(), 2);
        assert_eq!(ownership.body_kind, BodyKind::Sheet);

        graph.loops.get_mut(&2).unwrap().edges.push(3);
        assert_eq!(ownership_plan(&graph).unwrap().body_kind, BodyKind::General);
        graph.loops.get_mut(&2).unwrap().edges.pop();

        graph.loops.get_mut(&6).unwrap().edges[0] = 3;
        let ownership = ownership_plan(&graph).unwrap();
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
            edges,
            surface: 10,
        };
        let mut graph = B5Graph {
            complete: true,
            faces: Vec::new(),
            loops: BTreeMap::from([(1, loop_(1, vec![3])), (2, loop_(2, vec![4, 5, 3]))]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
            offset_surfaces: BTreeMap::new(),
            extrusion_surfaces: BTreeMap::new(),
            supported_surfaces: BTreeMap::new(),
            parameter_incidences: BTreeMap::new(),
            vertex_points: Vec::new(),
            logical_vertex_points: Vec::new(),
            logical_vertex_refs: Vec::new(),
            edge_vertices: BTreeMap::new(),
            edge_parameter_incidences: BTreeMap::new(),
            vertex_tolerances: BTreeMap::new(),
            profiles: BTreeMap::new(),
        };
        let orientation = orient_loop_members(
            &graph,
            BTreeMap::from([(1, vec![false]), (2, vec![false; 3])]),
        )
        .unwrap();
        assert_eq!(orientation[&1].member_order, vec![0]);
        assert_eq!(orientation[&2].member_order, vec![2, 1, 0]);
        assert_eq!(orientation[&1].reversed, vec![false]);
        assert_eq!(orientation[&2].reversed, vec![true; 3]);

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
            loops: BTreeMap::from([(
                1,
                B5Loop {
                    object_id: 1,
                    pcurves: vec![2],
                    edges: vec![3],
                    surface: 4,
                },
            )]),
            pcurves: BTreeMap::new(),
            opaque_pcurves: BTreeMap::new(),
            implicit_pcurves: BTreeMap::new(),
            surfaces: BTreeMap::new(),
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
    fn cylinder_pcurve_arc_length_normalizes_to_neutral_angle() {
        let surface = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
        };
        let point = neutral_pcurve_point([std::f64::consts::PI, 3.0], &surface);
        assert_eq!(point.u, std::f64::consts::FRAC_PI_2);
        assert_eq!(point.v, 3.0);
    }

    #[test]
    fn revolution_cache_preserves_native_profile_and_arc_length_chart() {
        let profile = B5Profile::Line {
            point: [2.0, 0.0, 0.0],
            direction: [0.0, 0.0, 1.0],
        };
        let (surface, plan) = revolution_surface(
            Some(&profile),
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            1.0,
            Some([[-1.0, 1.0], [0.0, std::f64::consts::PI]]),
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
            lifted_endpoints: None,
        };
        let plane = B5Surface::Plane {
            origin: [1.0, 2.0, 3.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
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
    fn affine_plane_lift_preserves_pcurve_weights() {
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 2,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![3, 3],
            control_points: vec![[1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            weights: Some(vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]),
            lifted_endpoints: None,
        };
        let plane = B5Surface::Plane {
            origin: [0.0, 0.0, 2.0],
            direction_u: [1.0, 0.0, 0.0],
            direction_v: [0.0, 1.0, 0.0],
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
    }

    #[test]
    fn isoparametric_circle_range_preserves_winding_and_seams() {
        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
        };
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 1.0],
            multiplicities: vec![2, 2],
            control_points: vec![[11.0, 3.0], [13.0, 3.0]],
            weights: None,
            lifted_endpoints: None,
        };
        let geometry = lifted_curve_geometry(&pcurve, &cylinder).expect("cylinder latitude");
        let edge_start = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            2.0,
            pcurve.control_points[0],
        );
        let edge_end = cylinder_point(
            [0.0; 3],
            [1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
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
        let turnback_end =
            cylinder_point([0.0; 3], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 2.0, [2.0, 3.0]);
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
            angular_offset: 0.0,
            slant_range: [-4.0, 0.0],
            angular_scale: 2.0,
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
            angular_offset: 0.0,
            slant_range: [2.0, 8.0],
            angular_scale: 3.0,
        };
        let pcurve = B5Pcurve {
            object_id: 1,
            surface: 2,
            degree: 1,
            distinct_knots: vec![0.0, 3.0 * std::f64::consts::PI],
            multiplicities: vec![2, 2],
            control_points: vec![[0.0, 4.0], [3.0 * std::f64::consts::PI, 4.0]],
            weights: None,
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
    fn torus_chart_lifts_meridians_and_latitudes_exactly() {
        let torus = B5Surface::Torus {
            center: [0.0, 0.0, 0.0],
            direction_x: [1.0, 0.0, 0.0],
            direction_y: [0.0, 1.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            major_radius: 5.0,
            minor_radius: 2.0,
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
            lifted_endpoints: None,
        };
        let cylinder = B5Surface::Cylinder {
            origin: [0.0, 0.0, 0.0],
            reference_x: [1.0, 0.0, 0.0],
            axis: [0.0, 0.0, 1.0],
            radius: 2.0,
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
    }
}
