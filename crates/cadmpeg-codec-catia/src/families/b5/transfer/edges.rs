// SPDX-License-Identifier: Apache-2.0
//! Edge-layer transfer: curve-plan merging, support resolution, and the
//! edge/curve/procedural-curve emit pass.

use std::collections::{BTreeMap, HashMap, HashSet};

use cadmpeg_ir::document::CadIr;
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
        ctx.insert_hash_map(plans, edge, candidate, "catia_b5_edge_curve_plans")?;
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
        ctx.insert_hash_set(conflicts, edge, "catia_b5_conflicting_edge_curves")?;
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
    surface_ids: &BTreeMap<u32, SurfaceId>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
    solved_parameter_range: Option<[f64; 2]>,
) -> Result<
    Option<(IdentityNamespace, &'static str, ProceduralCurveDefinition)>,
    cadmpeg_core::CodecError,
> {
    let ([first] | [first, _]) = supports else {
        return Ok(None);
    };
    if ctx
        .admit_iter(supports, "catia_b5_edge_support_domain_scan")?
        .any(|(_, pcurve, range)| {
            pcurves
                .get(pcurve)
                .is_none_or(|(_, _, domain)| bounded_occurrence_range(*range, *domain).is_none())
        })
    {
        return Ok(None);
    }
    let parameter_range = if let Some(parameter_range) = solved_parameter_range {
        parameter_range
    } else if first.2[0] < first.2[1]
        && ctx
            .admit_iter(supports, "catia_b5_edge_support_range_scan")?
            .skip(1)
            .all(|support| support.2 == first.2)
    {
        first.2.map(FiniteReal::get)
    } else {
        [0.0, 1.0]
    };
    let mut sides = std::array::from_fn(|_| IntcurveSupportSide {
        surface: None,
        pcurve: None,
    });
    for (support, side) in ctx
        .admit_iter(supports, "catia_b5_edge_support_definition_scan")?
        .zip(sides.iter_mut())
    {
        let (surface, pcurve, support_range) = support;
        let Some(surface_id) = surface_ids.get(surface) else {
            return Ok(None);
        };
        side.surface =
            Some(surface_id.try_clone_for_decode(ctx, "catia_b5_edge_support_surface_id")?);
        let support_range = support_range.map(FiniteReal::get);
        let mapped_range = (support_range != parameter_range)
            .then(|| DirectedParameterRange::new(support_range).ok())
            .flatten();
        let Some((geometry, _, _)) = pcurves.get(pcurve) else {
            return Ok(None);
        };
        side.pcurve = Some(SupportPcurve::new(
            geometry.try_clone_for_decode(ctx, "catia_b5_edge_support_pcurve")?,
            mapped_range,
        ));
    }
    let context = IntcurveSupportContext::try_new(
        sides,
        parameter_range,
        std::array::from_fn(|_| Vec::new()),
    )
    .ok();
    let Some(context) = context else {
        return Ok(None);
    };
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
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    supports: &[B5Support],
    endpoints: [[f64; 3]; 2],
    tolerances: [f64; 2],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> Result<bool, cadmpeg_core::CodecError> {
    for support in ctx.admit_iter(supports, "catia_b5_edge_support_follow_edge_scan")? {
        let Some([start, end]) = b5_support_endpoints(ctx, support, surfaces, pcurves)? else {
            return Ok(false);
        };
        if !(distance(start, endpoints[0]) <= tolerances[0]
            && distance(end, endpoints[1]) <= tolerances[1])
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn orient_b5_supports_to_edge(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    supports: &mut [B5Support],
    endpoints: [[f64; 3]; 2],
    tolerances: [f64; 2],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> Result<(), cadmpeg_core::CodecError> {
    for support in supports {
        let Some([start, end]) = b5_support_endpoints(ctx, support, surfaces, pcurves)? else {
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
    Ok(())
}

pub(super) fn b5_supports_agree(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    supports: &[B5Support],
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> Result<bool, cadmpeg_core::CodecError> {
    let mut supports = ctx.admit_iter(supports, "catia_b5_edge_support_agreement_scan")?;
    let Some(first) = supports.next() else {
        return Ok(false);
    };
    let Some(reference) = b5_support_endpoints(ctx, first, surfaces, pcurves)? else {
        return Ok(false);
    };
    for support in supports {
        let Some(candidate) = b5_support_endpoints(ctx, support, surfaces, pcurves)? else {
            return Ok(false);
        };
        let endpoint_error =
            distance(reference[0], candidate[0]).max(distance(reference[1], candidate[1]));
        if endpoint_error
            .partial_cmp(&EPS_SUPPORT_ENDPOINT)
            .is_none_or(std::cmp::Ordering::is_gt)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

pub(super) fn b5_support_endpoints(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    (surface, pcurve, range): &(u32, u32, [FiniteReal; 2]),
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> Result<Option<[[f64; 3]; 2]>, cadmpeg_core::CodecError> {
    let Some(surface) = surfaces.get(surface) else {
        return Ok(None);
    };
    let Some((pcurve, _, domain)) = pcurves.get(pcurve) else {
        return Ok(None);
    };
    if bounded_occurrence_range(*range, *domain).is_none() {
        return Ok(None);
    }
    let lifted = range.map(
        |parameter| -> Result<Option<[f64; 3]>, cadmpeg_core::CodecError> {
            let Some(uv) =
                cadmpeg_ir::eval::finite_or_refusal(cadmpeg_ir::eval::decode::outer_refusal(
                    cadmpeg_ir::eval::decode::pcurve_uv(ctx, pcurve, parameter.get()),
                )?)?
            else {
                return Ok(None);
            };
            // A non-finite support point is compared as a finite one is.
            let point = match cadmpeg_ir::eval::decode::outer_refusal(
                cadmpeg_ir::eval::decode::surface_point(ctx, &surface.geometry, uv.u, uv.v),
            )? {
                Ok(point) => point.get(),
                Err(failure) => match failure.non_finite()? {
                    Some(point) => point,
                    None => return Ok(None),
                },
            };
            Ok(Some([point.x, point.y, point.z]))
        },
    );
    let [start, end] = lifted;
    let [Some(start), Some(end)] = [start?, end?] else {
        return Ok(None);
    };
    Ok(Some([start, end]))
}

pub(super) fn b5_supports_follow_curve(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    supports: &[B5Support],
    curve: &CurvePlan,
    surfaces: &BTreeMap<u32, SurfacePlan>,
    pcurves: &BTreeMap<u32, (PcurveGeometry, bool, [FiniteReal; 2])>,
) -> Result<bool, cadmpeg_core::CodecError> {
    const EXACT_TOLERANCE: f64 = 1.0e-6;

    let Some(range) = curve_plan_parameter_range(curve) else {
        return Ok(false);
    };
    let solved = range.map(|parameter| -> Result<_, cadmpeg_core::CodecError> {
        Ok(cadmpeg_ir::eval::finite_or_refusal(
            cadmpeg_ir::eval::decode::outer_refusal(cadmpeg_ir::eval::decode::curve_point(
                ctx,
                &curve.geometry,
                parameter,
            ))?,
        )?)
    });
    let [start, end] = solved;
    let [Some(solved_start), Some(solved_end)] = [start?, end?] else {
        return Ok(false);
    };
    for support in ctx.admit_iter(supports, "catia_b5_edge_support_curve_scan")? {
        let Some([start, end]) = b5_support_endpoints(ctx, support, surfaces, pcurves)? else {
            return Ok(false);
        };
        let endpoint_error = distance([solved_start.x, solved_start.y, solved_start.z], start)
            .max(distance([solved_end.x, solved_end.y, solved_end.z], end));
        if endpoint_error
            .partial_cmp(&EXACT_TOLERANCE)
            .is_none_or(std::cmp::Ordering::is_gt)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// Emit the edges, their lifted 3D curves, and any procedural curve
/// definitions, returning the map from native edge id to emitted [`EdgeId`].
pub(super) fn emit_edges(
    ir: &mut CadIr,
    annotations: &mut AnnotationBuilder<impl cadmpeg_ir::annotations::AnnotationStorage>,
    graph: &B5Graph,
    payload: &cadmpeg_ir::ids::UnknownId,
    plan: &mut TransferPlan,
    surface_ids: &BTreeMap<u32, SurfaceId>,
    admission: &mut crate::families::FamilyEntityAdmission<'_, '_>,
) -> Result<HashMap<u32, EdgeId>, cadmpeg_core::CodecError> {
    let mut edge_id_map = HashMap::new();
    let edge_ids = std::mem::take(&mut plan.edge_ids);
    for &edge_id in admission
        .context()
        .admit_iter(&edge_ids, "catia_b5_emit_edge_ids")?
    {
        let index = usize::try_from(edge_id).map_err(|_| {
            admission
                .context()
                .refuse_codec_limit("catia_b5_edge_id", u64::MAX, u64::MAX)
        })?;
        let id = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "edge"),
            index,
            EdgeId::mint,
            "catia_b5_edge_id",
        )?;
        let curve_id = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "curve"),
            index,
            CurveId::mint,
            "catia_b5_edge_curve_id",
        )?;
        let endpoints = graph.vertices.edges()[&edge_id]
            .map(|vertex| vertex.combined_index(graph.vertices.raw_points().len()));
        let curve_plan =
            if let Some(plan) = plan.edge_curve_plan.remove(&edge_id) {
                plan
            } else {
                CurvePlan {
                    geometry: CurveGeometry::Solved(SolvedCurveGeometry::Unknown {
                        record: Some(payload.try_clone_for_decode(
                            admission.context(),
                            "catia_b5_edge_unknown_id",
                        )?),
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
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                curve_id.as_str(),
                "geometry",)?;
        }
        let model_curve_id =
            curve_id.try_clone_for_decode(admission.context(), "catia_b5_model_edge_curve_id")?;
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
            && plan.exact_support_curves.contains(&edge_id)
        {
            if let Some(supports) = plan.edge_support_plan.get(&edge_id) {
                b5_edge_support_definition(
                    admission.context(),
                    supports,
                    surface_ids,
                    &plan.pcurve_plan,
                    support_curve_range,
                )?
            } else {
                None
            }
        } else {
            None
        };
        if let Some((namespace, tag, definition)) = procedural {
            let procedural_id = crate::resource::compose_index_id(
                admission.context(),
                &namespace,
                index,
                ProceduralCurveId::mint,
                "catia_b5_edge_procedural_id",
            )?;
            annotate(
                admission.context(),
                annotations,
                &procedural_id,
                "object_stream_b5_03",
                tag,
                Exactness::Derived,
            )?;
            for field in ["curve", "definition"] {
                crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    procedural_id.as_str(),
                    field,)?;
            }
            if cache_fit_tolerance.is_some() {
                crate::resource::derived_annotation(
                    admission.context(),
                    annotations,
                    procedural_id.as_str(),
                    "cache_fit_tolerance",)?;
            }
            let mut definition = definition;
            if let Some(tolerance) = cache_fit_tolerance {
                definition
                    .set_legacy_cache(cadmpeg_ir::geometry::LegacyCache::new(tolerance))
                    .map_err(cadmpeg_core::CodecError::malformed)?;
            }
            let procedural = ProceduralCurve::new(procedural_id, definition);

            let owner = curve_id
                .try_clone_for_decode(admission.context(), "catia_b5_edge_procedural_owner_id")?;
            admission.charge()?;
            let _attached =
                ir.model
                    .add_procedural_curve(admission.context(), &owner, procedural)?;
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
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                id.as_str(),
                field,)?;
        }
        if edge_range.is_some() {
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                id.as_str(),
                "param_range",)?;
        }
        if edge_tolerance.is_some() {
            crate::resource::derived_annotation(
                admission.context(),
                annotations,
                id.as_str(),
                "tolerance",)?;
        }
        let map_id = id.try_clone_for_decode(admission.context(), "catia_b5_edge_map_id")?;
        admission.context().insert_hash_map(
            &mut edge_id_map,
            edge_id,
            map_id,
            "catia_b5_emitted_edge_ids",
        )?;
        let start = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "vertex"),
            endpoints[0],
            VertexId::mint,
            "catia_b5_edge_start_vertex_id",
        )?;
        let end = crate::resource::compose_index_id(
            admission.context(),
            &cadmpeg_ir::identity_namespace!("catia", "b5", "vertex"),
            endpoints[1],
            VertexId::mint,
            "catia_b5_edge_end_vertex_id",
        )?;
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

#[cfg(test)]
mod endpoint_admission_tests {
    use super::b5_support_endpoints;
    use crate::families::b5::transfer::SurfacePlan;
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    use std::collections::BTreeMap;

    #[test]
    fn b5_support_endpoints_propagate_caller_depth_refusal() {
        let surfaces = BTreeMap::from([(
            10,
            SurfacePlan {
                geometry: SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
                    cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                        Point3::new(0.0, 0.0, 0.0),
                        Vector3::new(0.0, 0.0, 1.0),
                        Vector3::new(1.0, 0.0, 0.0),
                    )
                    .expect("plane"),
                )),
                procedure: None,
            },
        )]);
        let range = crate::test_support::test_b5::finite_pair([0.0, 1.0]);
        let pcurves = BTreeMap::from([(
            20,
            (
                PcurveGeometry::Line(
                    LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0, 0.0))
                        .expect("line"),
                ),
                false,
                range,
            ),
        )]);
        crate::test_support::with_depth_limit(0, |ctx| {
            let error = b5_support_endpoints(ctx, &(10, 20, range), &surfaces, &pcurves)
                .expect_err("caller depth refuses surface evaluation");
            let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
                panic!("resource refusal required")
            };
            assert_eq!(
                limit.dimension,
                cadmpeg_core::decode::ResourceDimension::RecursionDepth
            );
            assert_eq!(ctx.resource_refusal(), Some(limit));
        });
    }
}
