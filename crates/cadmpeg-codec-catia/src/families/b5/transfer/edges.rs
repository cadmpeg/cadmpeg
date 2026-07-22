// SPDX-License-Identifier: Apache-2.0
//! Edge-layer transfer: curve-plan merging, support resolution, and the
//! edge/curve/procedural-curve emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{curve_point, pcurve_uv, surface_point};
use cadmpeg_ir::geometry::{
    Curve, CurveGeometry, IntcurveSupportContext, IntcurveSupportSide, PcurveGeometry,
    ProceduralCurve, ProceduralCurveDefinition, SurfaceCurveFamily,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, ProceduralCurveId, SurfaceId, VertexId};
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::B5Graph;
use super::{annotate, distance, B5Support, CurvePlan, SurfacePlan, TransferPlan};
use crate::native::cgm_source;

pub(super) fn merge_curve_plan(
    plans: &mut HashMap<u32, CurvePlan>,
    conflicts: &mut HashSet<u32>,
    edge: u32,
    candidate: CurvePlan,
) {
    if conflicts.contains(&edge) {
        return;
    }
    let Some(existing) = plans.get_mut(&edge) else {
        plans.insert(edge, candidate);
        return;
    };
    let range_conflict = existing
        .parameter_range
        .zip(candidate.parameter_range)
        .is_some_and(|(left, right)| left != right);
    let edge_tolerance_conflict = existing
        .edge_tolerance
        .zip(candidate.edge_tolerance)
        .is_some_and(|(left, right)| left != right);
    let cache_tolerance_conflict = existing
        .cache_fit_tolerance
        .zip(candidate.cache_fit_tolerance)
        .is_some_and(|(left, right)| left != right);
    if existing.geometry != candidate.geometry
        || range_conflict
        || edge_tolerance_conflict
        || cache_tolerance_conflict
    {
        plans.remove(&edge);
        conflicts.insert(edge);
        return;
    }
    if existing.parameter_range.is_none() {
        existing.parameter_range = candidate.parameter_range;
    }
    if existing.edge_tolerance.is_none() {
        existing.edge_tolerance = candidate.edge_tolerance;
    }
    if existing.cache_fit_tolerance.is_none() {
        existing.cache_fit_tolerance = candidate.cache_fit_tolerance;
    }
}

pub(super) fn curve_cache_has_ordered_knots(geometry: &CurveGeometry) -> bool {
    let CurveGeometry::Nurbs(curve) = geometry else {
        return true;
    };
    curve.knots.iter().all(|knot| knot.is_finite())
        && curve.knots.windows(2).all(|pair| pair[0] <= pair[1])
}

pub(super) fn curve_plan_parameter_range(plan: &CurvePlan) -> Option<[f64; 2]> {
    plan.parameter_range.or_else(|| {
        let CurveGeometry::Nurbs(curve) = &plan.geometry else {
            return None;
        };
        let degree = usize::try_from(curve.degree).ok()?;
        Some([
            *curve.knots.get(degree)?,
            *curve
                .knots
                .len()
                .checked_sub(degree + 1)
                .and_then(|index| curve.knots.get(index))?,
        ])
    })
}

pub(super) fn b5_vertex_point(graph: &B5Graph, vertex: usize) -> Option<[f64; 3]> {
    graph.vertex_points.get(vertex).copied().or_else(|| {
        vertex
            .checked_sub(graph.vertex_points.len())
            .and_then(|index| graph.logical_vertex_points.get(index))
            .copied()
    })
}

pub(super) fn edge_pcurve_parameters(graph: &B5Graph, edge: u32, pcurve: u32) -> Option<[f64; 2]> {
    graph
        .edge_parameter_incidences
        .get(&edge)?
        .map(|incidence_id| {
            let incidence = graph.parameter_incidences.get(&incidence_id)?;
            let mut parameters = incidence
                .curves
                .iter()
                .zip(&incidence.parameters)
                .filter_map(|(&curve, &parameter)| (curve == pcurve).then_some(parameter));
            let parameter = parameters.next()?;
            parameters
                .all(|other| other == parameter)
                .then_some(parameter)
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

pub(super) fn ordered_subrange(parameters: [f64; 2], domain: [f64; 2]) -> Option<[f64; 2]> {
    let parameters = bounded_occurrence_range(parameters, domain)?;
    Some(if parameters[0] < parameters[1] {
        parameters
    } else {
        [parameters[1], parameters[0]]
    })
}

pub(super) fn bounded_occurrence_range(parameters: [f64; 2], domain: [f64; 2]) -> Option<[f64; 2]> {
    const PARAMETER_TOLERANCE: f64 = 1e-9;

    if !parameters.into_iter().all(f64::is_finite)
        || parameters[0] == parameters[1]
        || parameters.iter().any(|parameter| {
            *parameter < domain[0] - PARAMETER_TOLERANCE
                || *parameter > domain[1] + PARAMETER_TOLERANCE
        })
    {
        return None;
    }
    Some(parameters.map(|parameter| parameter.clamp(domain[0], domain[1])))
}

pub(super) fn b5_edge_support_definition(
    supports: &[B5Support],
    surface_ids: &HashMap<u32, SurfaceId>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
    solved_parameter_range: Option<[f64; 2]>,
) -> Option<(&'static str, &'static str, ProceduralCurveDefinition)> {
    let ([first] | [first, _]) = supports else {
        return None;
    };
    if supports.iter().any(|(_, pcurve, range)| {
        pcurves
            .get(pcurve)
            .is_none_or(|(_, _, domain)| bounded_occurrence_range(*range, *domain).is_none())
    }) {
        return None;
    }
    let parameter_range = solved_parameter_range.unwrap_or_else(|| {
        if first.2[0] < first.2[1] && supports.iter().skip(1).all(|support| support.2 == first.2) {
            first.2
        } else {
            [0.0, 1.0]
        }
    });
    let mut sides = std::array::from_fn(|_| IntcurveSupportSide {
        surface: None,
        pcurve: None,
        pcurve_parameter_range: None,
    });
    for (side, (surface, pcurve, support_range)) in sides.iter_mut().zip(supports) {
        side.surface = Some(surface_ids.get(surface)?.clone());
        side.pcurve = Some(pcurves.get(pcurve)?.0.clone());
        side.pcurve_parameter_range = (*support_range != parameter_range).then_some(*support_range);
    }
    let context = IntcurveSupportContext {
        sides,
        parameter_range,
        discontinuities: std::array::from_fn(|_| Vec::new()),
    };
    if supports.len() == 2 && supports[0].0 != supports[1].0 {
        Some((
            "intersection",
            "two_surface_pcurve_intersection",
            ProceduralCurveDefinition::Intersection {
                context,
                discontinuity_flag: false,
            },
        ))
    } else {
        Some((
            "surface-curve",
            "parametric_surface_curve",
            ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric,
                context,
            },
        ))
    }
}

pub(super) fn b5_supports_follow_edge(
    supports: &[B5Support],
    endpoints: [[f64; 3]; 2],
    tolerances: [f64; 2],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
) -> bool {
    supports.iter().all(|support| {
        let Some([start, end]) = b5_support_endpoints(support, surfaces, pcurves) else {
            return false;
        };
        distance(start, endpoints[0]) <= tolerances[0]
            && distance(end, endpoints[1]) <= tolerances[1]
    })
}

pub(super) fn orient_b5_supports_to_edge(
    supports: &mut [B5Support],
    endpoints: [[f64; 3]; 2],
    tolerances: [f64; 2],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
) {
    for support in supports {
        let Some([start, end]) = b5_support_endpoints(support, surfaces, pcurves) else {
            continue;
        };
        let forward = distance(start, endpoints[0]) <= tolerances[0]
            && distance(end, endpoints[1]) <= tolerances[1];
        let reversed = distance(end, endpoints[0]) <= tolerances[0]
            && distance(start, endpoints[1]) <= tolerances[1];
        if !forward && reversed {
            support.2.swap(0, 1);
        }
    }
}

pub(super) fn b5_supports_agree(
    supports: &[B5Support],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
) -> bool {
    let mut lifted = supports
        .iter()
        .map(|support| b5_support_endpoints(support, surfaces, pcurves));
    let Some(Some(reference)) = lifted.next() else {
        return false;
    };
    lifted.all(|candidate| {
        candidate.is_some_and(|candidate| {
            distance(reference[0], candidate[0]).max(distance(reference[1], candidate[1])) <= 1e-6
        })
    })
}

pub(super) fn b5_support_endpoints(
    (surface, pcurve, range): &(u32, u32, [f64; 2]),
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
) -> Option<[[f64; 3]; 2]> {
    let surface = surfaces.get(surface)?;
    let (pcurve, _, domain) = pcurves.get(pcurve)?;
    bounded_occurrence_range(*range, *domain)?;
    let lifted = range.map(|parameter| {
        let uv = pcurve_uv(pcurve, parameter)?;
        let point = surface_point(&surface.geometry, uv.u, uv.v)?;
        Some([point.x, point.y, point.z])
    });
    let [Some(start), Some(end)] = lifted else {
        return None;
    };
    Some([start, end])
}

pub(super) fn b5_supports_follow_curve(
    supports: &[B5Support],
    curve: &CurvePlan,
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [f64; 2])>,
) -> bool {
    const EXACT_TOLERANCE: f64 = 1e-6;

    let Some(range) = curve_plan_parameter_range(curve) else {
        return false;
    };
    let solved = range.map(|parameter| curve_point(&curve.geometry, parameter));
    let [Some(solved_start), Some(solved_end)] = solved else {
        return false;
    };
    supports.iter().all(|support| {
        let Some([start, end]) = b5_support_endpoints(support, surfaces, pcurves) else {
            return false;
        };
        distance([solved_start.x, solved_start.y, solved_start.z], start)
            .max(distance([solved_end.x, solved_end.y, solved_end.z], end))
            <= EXACT_TOLERANCE
    })
}

/// Emit the edges, their lifted 3D curves, and any procedural curve
/// definitions, returning the map from native edge id to emitted [`EdgeId`].
pub(super) fn emit_edges(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder,
    graph: &B5Graph,
    payload: &cadmpeg_ir::ids::UnknownId,
    plan: &mut TransferPlan,
    surface_ids: &HashMap<u32, SurfaceId>,
) -> HashMap<u32, EdgeId> {
    let mut edge_id_map = HashMap::new();
    let edge_ids = std::mem::take(&mut plan.edge_ids);
    for edge_id in edge_ids {
        let id = EdgeId(format!("catia:b5:edge#{edge_id}"));
        let curve_id = CurveId(format!("catia:b5:curve#{edge_id}"));
        let endpoints = graph.edge_vertices[&edge_id];
        let mut curve_plan = plan
            .edge_curve_plan
            .remove(&edge_id)
            .unwrap_or_else(|| CurvePlan {
                geometry: CurveGeometry::Unknown {
                    record: Some(payload.clone()),
                },
                parameter_range: None,
                edge_tolerance: None,
                cache_fit_tolerance: None,
            });
        if !curve_cache_has_ordered_knots(&curve_plan.geometry) {
            curve_plan = CurvePlan {
                geometry: CurveGeometry::Unknown {
                    record: Some(payload.clone()),
                },
                parameter_range: None,
                edge_tolerance: None,
                cache_fit_tolerance: None,
            };
            plan.edge_helix_plan.remove(&edge_id);
        }
        let helix = plan.edge_helix_plan.remove(&edge_id);
        let edge_range = curve_plan.parameter_range;
        let support_curve_range = curve_plan_parameter_range(&curve_plan);
        let edge_tolerance = curve_plan.edge_tolerance;
        let cache_fit_tolerance = curve_plan.cache_fit_tolerance;
        let geometry = curve_plan.geometry;
        annotate(
            annotations,
            &curve_id,
            "object_stream_b5_03",
            "pcurve_lifted_3d_curve",
            if matches!(geometry, CurveGeometry::Unknown { .. }) {
                Exactness::Unknown
            } else {
                Exactness::Derived
            },
        );
        if !matches!(geometry, CurveGeometry::Unknown { .. }) {
            annotations.derived(&curve_id, "geometry");
        }
        ir.model.curves.push(Curve {
            id: curve_id.clone(),
            geometry,
            source_object: Some(cgm_source("edge", edge_id)),
        });
        let procedural = helix
            .as_ref()
            .map(|plan| {
                (
                    "helix",
                    "cylinder_parametric_helix",
                    plan.definition.clone(),
                )
            })
            .or_else(|| {
                let supports = plan.edge_support_plan.get(&edge_id)?;
                if !plan.exact_support_edges.contains(&edge_id)
                    || !plan.exact_support_curves.contains(&edge_id)
                {
                    return None;
                }
                b5_edge_support_definition(
                    supports,
                    surface_ids,
                    &plan.pcurve_plan,
                    support_curve_range,
                )
            });
        if let Some((kind, tag, definition)) = procedural {
            let procedural_id = ProceduralCurveId(format!("catia:b5:{kind}#{edge_id}"));
            annotate(
                annotations,
                &procedural_id,
                "object_stream_b5_03",
                tag,
                Exactness::Derived,
            );
            annotations
                .derived(&procedural_id, "curve")
                .derived(&procedural_id, "definition");
            if cache_fit_tolerance.is_some() {
                annotations.derived(&procedural_id, "cache_fit_tolerance");
            }
            ir.model.procedural_curves.push(ProceduralCurve {
                id: procedural_id,
                curve: curve_id.clone(),
                definition,
                cache_fit_tolerance,
            });
        }
        annotate(
            annotations,
            &id,
            "object_stream_b5_03",
            "5e_edge",
            Exactness::ByteExact,
        );
        annotations.derived(&id, "start").derived(&id, "end");
        if edge_range.is_some() {
            annotations.derived(&id, "param_range");
        }
        if edge_tolerance.is_some() {
            annotations.derived(&id, "tolerance");
        }
        edge_id_map.insert(edge_id, id.clone());
        ir.model.edges.push(Edge {
            id,
            curve: Some(curve_id),
            start: VertexId(format!("catia:b5:vertex#{}", endpoints[0])),
            end: VertexId(format!("catia:b5:vertex#{}", endpoints[1])),
            param_range: edge_range,
            tolerance: edge_tolerance,
        });
    }
    edge_id_map
}
