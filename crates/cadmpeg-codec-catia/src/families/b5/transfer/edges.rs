// SPDX-License-Identifier: Apache-2.0
//! Edge-layer transfer: curve-plan merging, support resolution, and the
//! edge/curve/procedural-curve emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::eval::{curve_point, pcurve_uv, surface_point};
use cadmpeg_ir::geometry::{
    pcurve::PcurveGeometry, Curve, CurveGeometry, DirectedParameterRange, IntcurveSupportContext,
    IntcurveSupportSide, ProceduralCurve, ProceduralCurveDefinition, SolvedCurveGeometry,
    SupportPcurve, SurfaceCurveFamily,
};
use cadmpeg_ir::ids::{CurveId, EdgeId, IdentityNamespace, ProceduralCurveId, SurfaceId, VertexId};
use cadmpeg_ir::scalar::FiniteReal;
use cadmpeg_ir::topology::Edge;
use cadmpeg_ir::{AnnotationBuilder, Exactness};

use super::super::graph::{bounded_occurrence_range, B5Graph};
use super::{annotate, B5Support, CurvePlan, SurfacePlan, TransferPlan};
use crate::assemble::cgm_source;
use crate::math::distance;

const EPS_SUPPORT_ENDPOINT: f64 = 1.0e-6;

pub(super) fn merge_curve_plan(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    plans: &mut HashMap<u32, CurvePlan>,
    conflicts: &mut HashSet<u32>,
    edge: u32,
    candidate: CurvePlan,
) -> Result<(), cadmpeg_core::CodecError> {
    if conflicts.contains(&edge) {
        return Ok(());
    }
    let Some(existing) = plans.get_mut(&edge) else {
        crate::resource::insert_map(ctx, plans, edge, candidate,
            "catia_b5_edge_curve_plans")?;
        return Ok(());
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
        crate::resource::insert_set(ctx, conflicts, edge,
            "catia_b5_conflicting_edge_curves")?;
        plans.remove(&edge);
        return Ok(());
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
    Ok(())
}

fn curve_plan_parameter_range(plan: &CurvePlan) -> Option<[f64; 2]> {
    plan.parameter_range.or_else(|| {
        let Some(SolvedCurveGeometry::Nurbs(curve)) = plan.geometry.solved() else {
            return None;
        };
        let degree = usize::try_from(curve.degree()).ok()?;
        Some([
            *curve.knots().get(degree)?,
            *curve
                .knots()
                .len()
                .checked_sub(degree + 1)
                .and_then(|index| curve.knots().get(index))?,
        ])
    })
}

pub(super) fn ordered_subrange(
    parameters: [FiniteReal; 2],
    domain: [FiniteReal; 2],
) -> Option<[FiniteReal; 2]> {
    let parameters = bounded_occurrence_range(parameters, domain)?;
    Some(if parameters[0] < parameters[1] {
        parameters
    } else {
        [parameters[1], parameters[0]]
    })
}

pub(super) fn b5_edge_support_definition(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    supports: &[B5Support],
    surface_ids: &HashMap<u32, SurfaceId>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
    solved_parameter_range: Option<[f64; 2]>,
) -> Result<Option<(IdentityNamespace, &'static str, ProceduralCurveDefinition)>, cadmpeg_core::CodecError> {
    let ([first] | [first, _]) = supports else {
        return Ok(None);
    };
    if supports.iter().any(|(_, pcurve, range)| {
        pcurves
            .get(pcurve)
            .is_none_or(|(_, _, domain)| bounded_occurrence_range(*range, *domain).is_none())
    }) {
        return Ok(None);
    }
    let parameter_range = solved_parameter_range.unwrap_or_else(|| {
        if first.2[0] < first.2[1] && supports.iter().skip(1).all(|support| support.2 == first.2) {
            first.2.map(FiniteReal::get)
        } else {
            [0.0, 1.0]
        }
    });
    let mut sides = std::array::from_fn(|_| IntcurveSupportSide {
        surface: None,
        pcurve: None,
    });
    for (side, (surface, pcurve, support_range)) in sides.iter_mut().zip(supports) {
        let Some(surface_id) = surface_ids.get(surface) else { return Ok(None) };
        side.surface = Some(crate::resource::copy_id(ctx, surface_id.as_str(), SurfaceId::mint,
            "catia_b5_edge_support_surface_id")?);
        let support_range = support_range.map(FiniteReal::get);
        let mapped_range = (support_range != parameter_range)
            .then(|| DirectedParameterRange::new(support_range).ok())
            .flatten();
        let Some((geometry, _, _)) = pcurves.get(pcurve) else { return Ok(None) };
        side.pcurve = Some(SupportPcurve::new(
            crate::resource::copy_pcurve_geometry(ctx, geometry,
                "catia_b5_edge_support_pcurve")?,
            mapped_range,
        ));
    }
    let context = IntcurveSupportContext::try_new(
        sides,
        parameter_range,
        std::array::from_fn(|_| Vec::new()),
    )
    .ok();
    let Some(context) = context else { return Ok(None) };
    if supports.len() == 2 && supports[0].0 != supports[1].0 {
        Ok(Some((
            cadmpeg_ir::identity_namespace!("catia", "b5", "intersection"),
            "two_surface_pcurve_intersection",
            ProceduralCurveDefinition::Intersection {
                context,
                discontinuity_flag: false,
                cache: None,
            },
        )))
    } else {
        Ok(Some((
            cadmpeg_ir::identity_namespace!("catia", "b5", "surface-curve"),
            "parametric_surface_curve",
            ProceduralCurveDefinition::SurfaceCurve {
                family: SurfaceCurveFamily::Parametric {
                    context,
                    tail: None,
                },
            },
        )))
    }
}

pub(super) fn b5_supports_follow_edge(
    supports: &[B5Support],
    endpoints: [[f64; 3]; 2],
    tolerances: [f64; 2],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
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
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) {
    for support in supports {
        let Some([start, end]) = b5_support_endpoints(support, surfaces, pcurves) else {
            continue;
        };
        let forward_residuals = [distance(start, endpoints[0]), distance(end, endpoints[1])];
        let reversed_residuals = [distance(end, endpoints[0]), distance(start, endpoints[1])];
        let forward = forward_residuals
            .iter()
            .zip(tolerances)
            .all(|(residual, tolerance)| *residual <= tolerance);
        let reversed = reversed_residuals
            .iter()
            .zip(tolerances)
            .all(|(residual, tolerance)| *residual <= tolerance);
        let reversed_is_closer = reversed_residuals[0].max(reversed_residuals[1])
            < forward_residuals[0].max(forward_residuals[1]);
        if reversed && (!forward || reversed_is_closer) {
            support.2.swap(0, 1);
        }
    }
}

pub(super) fn b5_supports_agree(
    supports: &[B5Support],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> bool {
    let mut lifted = supports
        .iter()
        .map(|support| b5_support_endpoints(support, surfaces, pcurves));
    let Some(Some(reference)) = lifted.next() else {
        return false;
    };
    lifted.all(|candidate| {
        candidate.is_some_and(|candidate| {
            distance(reference[0], candidate[0]).max(distance(reference[1], candidate[1]))
                <= EPS_SUPPORT_ENDPOINT
        })
    })
}

pub(super) fn b5_support_endpoints(
    (surface, pcurve, range): &(u32, u32, [FiniteReal; 2]),
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> Option<[[f64; 3]; 2]> {
    let surface = surfaces.get(surface)?;
    let (pcurve, _, domain) = pcurves.get(pcurve)?;
    bounded_occurrence_range(*range, *domain)?;
    let lifted = range.map(|parameter| {
        let uv = pcurve_uv(pcurve, parameter.get()).ok()?;
        // A non-finite support point is compared as a finite one is.
        let point = match surface_point(&surface.geometry, uv.u, uv.v) {
            Ok(point) => point.get(),
            Err(failure) => failure.non_finite()?,
        };
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
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> bool {
    const EXACT_TOLERANCE: f64 = 1.0e-6;

    let Some(range) = curve_plan_parameter_range(curve) else {
        return false;
    };
    let solved = range.map(|parameter| curve_point(&curve.geometry, parameter).ok());
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
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<HashMap<u32, EdgeId>, cadmpeg_core::CodecError> {
    let mut edge_id_map = HashMap::new();
    let edge_ids = std::mem::take(&mut plan.edge_ids);
    for edge_id in edge_ids {
        let index = usize::try_from(edge_id).map_err(|_|
            admission.context().refuse_codec_limit("catia_b5_edge_id", u64::MAX, u64::MAX))?;
        let id = crate::resource::compose_index_id(admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "edge"),
            index, EdgeId::mint, "catia_b5_edge_id")?;
        let curve_id = crate::resource::compose_index_id(admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "curve"),
            index, CurveId::mint, "catia_b5_edge_curve_id")?;
        let endpoints = graph.vertices.edges()[&edge_id]
            .map(|vertex| vertex.combined_index(graph.vertices.raw_points().len()));
        let curve_plan = if let Some(plan) = plan.edge_curve_plan.remove(&edge_id) {
            plan
        } else {
            CurvePlan {
                geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                    record: Some(crate::resource::copy_id(admission.context(), payload.as_str(),
                        cadmpeg_ir::ids::UnknownId::mint, "catia_b5_edge_unknown_id")?),
                }),
                parameter_range: None,
                edge_tolerance: None,
                cache_fit_tolerance: None,
            }
        };

        let helix = plan.edge_helix_plan.remove(&edge_id);
        let edge_range = curve_plan.parameter_range;
        let support_curve_range = curve_plan_parameter_range(&curve_plan);
        let edge_tolerance = curve_plan.edge_tolerance;
        let cache_fit_tolerance = curve_plan.cache_fit_tolerance;
        let geometry = curve_plan.geometry;
        annotate(
            admission.context(),
            annotations,
            &curve_id,
            "object_stream_b5_03",
            "pcurve_lifted_3d_curve",
            if matches!(
                geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
            ) {
                Exactness::Unknown
            } else {
                Exactness::Derived
            },
        )?;
        if !matches!(
            geometry,
            CurveGeometry::Solved(SolvedCurveGeometry::Unknown { .. })
        ) {
            crate::resource::derived_annotation(admission.context(), annotations,
                curve_id.as_str(), "geometry", "catia_b5_curve_geometry_annotation")?;
        }
        let model_curve_id = crate::resource::copy_id(admission.context(), curve_id.as_str(),
            CurveId::mint, "catia_b5_model_edge_curve_id")?;
        admission.reserve_entity(&mut ir.model.curves, "catia_b5_emit_curves")?;
        ir.model.curves.push(Curve {
            id: model_curve_id,
            geometry,
            source_object: Some(cgm_source(admission.context(), "edge", edge_id)?),
        });
        let procedural = if let Some(helix) = helix {
            Some((
                    cadmpeg_ir::identity_namespace!("catia", "b5", "helix"),
                    "cylinder_parametric_helix",
                    helix.definition,
            ))
        } else if plan.exact_support_edges.contains(&edge_id)
            && plan.exact_support_curves.contains(&edge_id) {
            if let Some(supports) = plan.edge_support_plan.get(&edge_id) {
                b5_edge_support_definition(
                    admission.context(),
                    supports,
                    surface_ids,
                    &plan.pcurve_plan,
                    support_curve_range,
                )?
            } else { None }
        } else { None };
        if let Some((namespace, tag, definition)) = procedural {
            let procedural_id = crate::resource::compose_index_id(admission.context(),
                &namespace, index, ProceduralCurveId::mint,
                "catia_b5_edge_procedural_id")?;
            annotate(
                admission.context(),
                annotations,
                &procedural_id,
                "object_stream_b5_03",
                tag,
                Exactness::Derived,
            )?;
            for field in ["curve", "definition"] {
                crate::resource::derived_annotation(admission.context(), annotations,
                    procedural_id.as_str(), field, "catia_b5_procedural_curve_annotation")?;
            }
            if cache_fit_tolerance.is_some() {
                crate::resource::derived_annotation(admission.context(), annotations,
                    procedural_id.as_str(), "cache_fit_tolerance",
                    "catia_b5_procedural_curve_annotation")?;
            }
            let mut definition = definition;
            if let Some(tolerance) = cache_fit_tolerance {
                definition
                    .set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::new(tolerance))
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            let procedural = ProceduralCurve::new(procedural_id, definition);

            let owner = crate::resource::copy_id(admission.context(), curve_id.as_str(),
                CurveId::mint, "catia_b5_edge_procedural_owner_id")?;
            admission.reserve_entity(&mut ir.model.procedural_curves, "catia_b5_emit_procedural_curves")?;
            let _attached = ir.model.add_procedural_curve(owner, procedural);
        }
        annotate(
            admission.context(),
            annotations,
            &id,
            "object_stream_b5_03",
            "5e_edge",
            Exactness::ByteExact,
        )?;
        for field in ["start", "end"] {
            crate::resource::derived_annotation(admission.context(), annotations,
                id.as_str(), field, "catia_b5_edge_annotation")?;
        }
        if edge_range.is_some() {
            crate::resource::derived_annotation(admission.context(), annotations,
                id.as_str(), "param_range", "catia_b5_edge_annotation")?;
        }
        if edge_tolerance.is_some() {
            crate::resource::derived_annotation(admission.context(), annotations,
                id.as_str(), "tolerance", "catia_b5_edge_annotation")?;
        }
        let map_id = crate::resource::copy_id(admission.context(), id.as_str(),
            EdgeId::mint, "catia_b5_edge_map_id")?;
        crate::resource::insert_map(admission.context(), &mut edge_id_map, edge_id, map_id,
            "catia_b5_emitted_edge_ids")?;
        let start = crate::resource::compose_index_id(admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "vertex"),
            endpoints[0], VertexId::mint, "catia_b5_edge_start_vertex_id")?;
        let end = crate::resource::compose_index_id(admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "vertex"),
            endpoints[1], VertexId::mint, "catia_b5_edge_end_vertex_id")?;
        admission.reserve_entity(&mut ir.model.edges, "catia_b5_emit_edges")?;
        ir.model.edges.push(Edge {
            id,
            carrier: cadmpeg_ir::topology::EdgeCarrier::new(Some(curve_id), edge_range)
                .map_err(cadmpeg_core::CodecError::malformed)?,
            start,
            end,
            tolerance: edge_tolerance,
        });
    }
    Ok(edge_id_map)
}
