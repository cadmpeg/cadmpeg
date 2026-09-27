// SPDX-License-Identifier: Apache-2.0
//! Model surface point evaluation across stored and procedural carriers.

use super::depth::MAX_MODEL_EVALUATION_DEPTH;
use super::{
    cacheless_constant_rolling_ball_first_order, cacheless_constant_rolling_ball_point,
    cacheless_law_sweep_point, cacheless_variable_blend_point, model_axis_revolution_point,
    model_linear_sweep_point, model_native_extrusion_point, model_native_revolution_point,
    model_ruled_surface_jet, model_sum_surface_jet, model_surface_jet_by_id,
    model_surface_point_with_budget, offset, placed_reach, placed_vectors, record_u_interval,
    revision_surface_tail_has_current_cache, rolling_ball_jet_point, scale_vector,
    subset_support_parameters_with_derivatives, surface_first_order, surface_point,
    surface_point_with_budget, sweep_has_current_cache, unit_cross_direction,
    variable_blend_has_current_cache, EvaluationFailure, ModelEvaluationDepthGuard,
    UNREACHED_POINT,
};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::{
    ProceduralSurfaceDefinition, SolvedSurfaceGeometry, Surface, SurfaceGeometry,
};
use crate::math::{Point3, Vector3};
use cadmpeg_core::decode::{ResourceLimit, WorkBudget};

pub(super) fn model_surface_point_by_id_inner(
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
    budget: Option<&WorkBudget<'_>>,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    /// An arm's point, admitted where the arm computes it raw, and the
    /// support's oriented unit normal for an offset that reads it.
    struct SurfaceEvaluation {
        /// The point, or the non-finite point the arm reached.
        point: Result<FinitePoint3, Point3>,
        /// The oriented unit normal, or why it has none: an arm that forms
        /// no normal, and an evaluation whose reader reads none, have no
        /// value.
        oriented_normal: Result<Vector3, EvaluationFailure<()>>,
        /// Refusal of scratch used by the selected carrier or support.
        resource: Option<ResourceLimit>,
    }

    fn resource(limit: ResourceLimit) -> SurfaceEvaluation {
        SurfaceEvaluation {
            point: Err(UNREACHED_POINT),
            oriented_normal: Err(EvaluationFailure::ResourceLimit(limit)),
            resource: Some(limit),
        }
    }

    /// Admit a point an arm computes raw, keeping the non-finite point.
    fn evaluated(point: Point3) -> Result<FinitePoint3, Point3> {
        FinitePoint3::new(point).ok_or(point)
    }

    /// The point of an evaluation, finite or not.
    fn reached(point: Result<FinitePoint3, Point3>) -> Point3 {
        point.map_or_else(|point| point, FinitePoint3::get)
    }

    /// The evaluation of an arm that forms its point alone.
    fn point_evaluation(
        point: Result<FinitePoint3, EvaluationFailure<Point3>>,
    ) -> Option<SurfaceEvaluation> {
        let point = match point {
            Ok(point) => Ok(point),
            Err(EvaluationFailure::NonFinite(point)) => Err(point),
            Err(EvaluationFailure::NoValue) => return None,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
        };
        Some(SurfaceEvaluation {
            point,
            oriented_normal: Err(EvaluationFailure::NoValue),
            resource: None,
        })
    }

    /// The evaluation of a construction that falls back to its current
    /// cache: the cache's point where it has one, otherwise the construction's
    /// point outside the finite range, otherwise the cache's point outside the
    /// finite range.
    fn cache_fallback(
        construction: EvaluationFailure<Point3>,
        cached: Option<SurfaceEvaluation>,
    ) -> Option<SurfaceEvaluation> {
        if let Some(limit) = cached.as_ref().and_then(|evaluation| evaluation.resource) {
            return Some(resource(limit));
        }
        match (cached, construction) {
            (_, EvaluationFailure::ResourceLimit(limit)) => Some(resource(limit)),
            (Some(cached), _) if cached.point.is_ok() => Some(cached),
            (_, EvaluationFailure::NonFinite(point)) => Some(SurfaceEvaluation {
                point: Err(point),
                oriented_normal: Err(EvaluationFailure::NonFinite(())),
                resource: None,
            }),
            (cached, EvaluationFailure::NoValue) => cached,
        }
    }

    /// The oriented unit normal of first partials: their cross product,
    /// reversed where `reversed` is set. A normal whose direction is zero
    /// has no value.
    fn oriented_normal(
        first: Result<[FiniteVector3; 2], EvaluationFailure<()>>,
        reversed: bool,
    ) -> Result<Vector3, EvaluationFailure<()>> {
        let [du, dv] = first?;
        let cross = du.get().cross(dv.get());
        let magnitude = cross.norm();
        let normal = if magnitude.is_finite() && magnitude > 0.0 {
            Vector3::new(
                cross.x / magnitude,
                cross.y / magnitude,
                cross.z / magnitude,
            )
        } else {
            unit_cross_direction(du, dv)?
        };
        Ok(if reversed {
            scale_vector(normal, -1.0)
        } else {
            normal
        })
    }

    /// A stored cache's point and unit normal. The point is evaluated alone;
    /// the normal is that of the cache's first partials.
    fn cache_evaluation(geometry: &SurfaceGeometry, u: f64, v: f64) -> Option<SurfaceEvaluation> {
        let point = match surface_point(geometry, u, v) {
            Ok(point) => Ok(point),
            Err(EvaluationFailure::NonFinite(point)) => Err(point),
            Err(EvaluationFailure::NoValue) => return None,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
        };
        let oriented_normal = surface_first_order(geometry, u, v, None)
            .map_err(|failure| failure.map(|_| ()))
            .and_then(|order| {
                let [du, dv] = order.first?;
                du.get()
                    .cross(dv.get())
                    .unit()
                    .ok_or(EvaluationFailure::NoValue)
            });
        if let Err(EvaluationFailure::ResourceLimit(limit)) = oriented_normal {
            return Some(resource(limit));
        }
        Some(SurfaceEvaluation {
            point,
            oriented_normal,
            resource: None,
        })
    }

    /// A directly stored carrier's point and, for a reader that reads it,
    /// its oriented unit normal. The point is evaluated alone where no normal
    /// is read; otherwise it is the point of the first partials, which leave
    /// the point finite where they leave the finite range.
    fn direct_evaluation(
        geometry: &SurfaceGeometry,
        u: f64,
        v: f64,
        budget: Option<&WorkBudget<'_>>,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        if !normal {
            return point_evaluation(match budget {
                Some(budget) => surface_point_with_budget(geometry, u, v, budget),
                None => surface_point(geometry, u, v),
            });
        }
        let order = match surface_first_order(geometry, u, v, budget) {
            Ok(order) => order,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
            Err(EvaluationFailure::NoValue) => return None,
            Err(EvaluationFailure::NonFinite(point)) => {
                return Some(SurfaceEvaluation {
                    point: Err(point),
                    oriented_normal: Err(EvaluationFailure::NonFinite(())),
                    resource: None,
                })
            }
        };
        let reversed = matches!(
            geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) if nurbs.normal_reversed()
        );
        let oriented_normal = oriented_normal(order.first, reversed);
        if let Err(EvaluationFailure::ResourceLimit(limit)) = oriented_normal {
            return Some(resource(limit));
        }
        Some(SurfaceEvaluation {
            point: Ok(order.point),
            oriented_normal,
            resource: None,
        })
    }

    /// The offset of a support whose normal has no value: a support point
    /// outside the finite range, and a normal outside it, reach no offset
    /// coordinate; a finite support point without a normal has no offset.
    fn offset_without_normal(
        support: &SurfaceEvaluation,
        failure: EvaluationFailure<()>,
    ) -> Option<SurfaceEvaluation> {
        if let EvaluationFailure::ResourceLimit(limit) = failure {
            return Some(resource(limit));
        }
        if let Some(limit) = support.resource {
            return Some(resource(limit));
        }
        (support.point.is_err() || failure == EvaluationFailure::NonFinite(())).then_some(
            SurfaceEvaluation {
                point: Err(Point3::new(f64::NAN, f64::NAN, f64::NAN)),
                oriented_normal: Err(EvaluationFailure::NonFinite(())),
                resource: None,
            },
        )
    }

    fn linear_nurbs_support_extension(
        index: &crate::index::ModelIndex<'_>,
        support: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        budget: Option<&WorkBudget<'_>>,
    ) -> Option<SurfaceEvaluation> {
        let support = index.surfaces(support.as_str())?;
        let Some(SolvedSurfaceGeometry::Nurbs(nurbs)) = support.geometry.solved() else {
            return None;
        };
        let u_degree = usize::try_from(nurbs.u_degree()).ok()?;
        let v_degree = usize::try_from(nurbs.v_degree()).ok()?;
        let u_count = nurbs.u_count();
        let v_count = nurbs.v_count();
        let u_domain = [
            *nurbs.u_knots().get(u_degree)?,
            *nurbs.u_knots().get(u_count)?,
        ];
        let v_domain = [
            *nurbs.v_knots().get(v_degree)?,
            *nurbs.v_knots().get(v_count)?,
        ];
        let boundary = |value: f64, [lower, upper]: [f64; 2]| {
            if !value.is_finite() || !lower.is_finite() || !upper.is_finite() || lower > upper {
                return None;
            }
            Some(if value < lower {
                (lower, true)
            } else if value > upper {
                (upper, true)
            } else {
                (value, false)
            })
        };
        let (boundary_u, u_extended) = boundary(u, u_domain)?;
        let (boundary_v, v_extended) = boundary(v, v_domain)?;
        if !u_extended && !v_extended {
            return None;
        }
        let order = match surface_first_order(&support.geometry, boundary_u, boundary_v, budget) {
            Ok(order) => order,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return None,
        };
        let partials = match order.partials() {
            Ok(partials) => partials,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return None,
        };
        let oriented_normal =
            match oriented_normal(Ok([partials.du, partials.dv]), nurbs.normal_reversed()) {
                Ok(normal) => normal,
                Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
                Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(())) => return None,
            };
        let partials = partials.into_raw();
        let du = u - boundary_u;
        let dv = v - boundary_v;
        Some(SurfaceEvaluation {
            point: evaluated(Point3::new(
                partials.point.x + du * partials.du.x + dv * partials.dv.x,
                partials.point.y + du * partials.du.y + dv * partials.dv.y,
                partials.point.z + du * partials.du.z + dv * partials.dv.z,
            )),
            oriented_normal: Ok(oriented_normal),
            resource: None,
        })
    }

    /// The evaluation of `surface_id` at `(u, v)`, with the oriented normal
    /// where `normal` is set: an offset reads its support's normal, and a
    /// placement or subset passes its reader's need on to its support.
    fn evaluate(
        index: &crate::index::ModelIndex<'_>,
        surface_id: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        visiting: &mut [Option<*const Surface>; MAX_MODEL_EVALUATION_DEPTH],
        visit_depth: usize,
        budget: Option<&WorkBudget<'_>>,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        let _depth = ModelEvaluationDepthGuard::enter(budget)?;
        if let Some(budget) = budget {
            budget.charge().then_some(())?;
        }
        let surface = index.surfaces(surface_id.as_str())?;
        let identity = surface as *const Surface;
        if visiting[..visit_depth].contains(&Some(identity)) {
            return None;
        }
        *visiting.get_mut(visit_depth)? = Some(identity);
        let next_depth = visit_depth + 1;
        let procedural = index.procedural_surface_for_surface(surface_id.as_str());
        let carrier_interval =
            procedural.and_then(|procedural| record_u_interval(procedural.record_bounds()));
        let result = match procedural.map(crate::geometry::ProceduralSurface::definition) {
            Some(ProceduralSurfaceDefinition::AxisRevolution(definition_payload)) => {
                point_evaluation(model_axis_revolution_point(
                    index,
                    definition_payload.directrix(),
                    definition_payload.axis_origin().get(),
                    definition_payload.axis_direction(),
                    u,
                    v,
                    budget,
                ))
            }
            Some(ProceduralSurfaceDefinition::Extrusion(definition_payload)) => {
                point_evaluation(model_native_extrusion_point(
                    index,
                    definition_payload,
                    carrier_interval,
                    u,
                    v,
                    budget,
                ))
            }
            Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => point_evaluation(
                model_linear_sweep_point(index, definition_payload, u, v, budget),
            ),
            Some(ProceduralSurfaceDefinition::Revolution(definition_payload)) => {
                point_evaluation(model_native_revolution_point(
                    index,
                    definition_payload,
                    carrier_interval,
                    u,
                    v,
                    budget,
                ))
            }
            Some(ProceduralSurfaceDefinition::Ruled { first, second, .. }) => point_evaluation(
                model_ruled_surface_jet(index, first, second, u, v).map(|jet| jet.point),
            ),
            Some(ProceduralSurfaceDefinition::Sum(definition_payload)) => point_evaluation(
                model_sum_surface_jet(index, definition_payload, u, v).map(|jet| jet.point),
            ),
            Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) => {
                if let Some(construction) = definition_payload.native() {
                    let profile = definition_payload.profile();
                    let spine = definition_payload.spine();

                    match cacheless_law_sweep_point(index, profile, spine, construction, u, v) {
                        Ok(point) => Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal: Err(EvaluationFailure::NoValue),
                            resource: None,
                        }),
                        Err(failure) => cache_fallback(
                            failure,
                            sweep_has_current_cache(construction)
                                .then(|| cache_evaluation(&surface.geometry, u, v))
                                .flatten(),
                        ),
                    }
                } else {
                    direct_evaluation(&surface.geometry, u, v, budget, normal)
                }
            }
            Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) => {
                let construction = definition_payload.construction();

                match cacheless_variable_blend_point(index, definition_payload, u, v) {
                    Ok(point) => Some(SurfaceEvaluation {
                        point: evaluated(point),
                        oriented_normal: Err(EvaluationFailure::NoValue),
                        resource: None,
                    }),
                    Err(failure) => cache_fallback(
                        failure,
                        variable_blend_has_current_cache(construction)
                            .then(|| cache_evaluation(&surface.geometry, u, v))
                            .flatten(),
                    ),
                }
            }
            Some(ProceduralSurfaceDefinition::Blend(definition_payload)) => {
                if let Some(native) = definition_payload.native() {
                    match cacheless_constant_rolling_ball_point(index, definition_payload, u, v) {
                        Ok(point) => Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal: if normal {
                                cacheless_constant_rolling_ball_first_order(
                                    index,
                                    definition_payload,
                                    u,
                                    v,
                                )
                                .map_err(|failure| failure.map(|_| ()))
                                .and_then(|order| {
                                    let [du, dv] = order.first?;
                                    du.get()
                                        .cross(dv.get())
                                        .unit()
                                        .ok_or(EvaluationFailure::NoValue)
                                })
                            } else {
                                Err(EvaluationFailure::NoValue)
                            },
                            resource: None,
                        }),
                        Err(failure) => cache_fallback(
                            failure.map(|()| UNREACHED_POINT),
                            revision_surface_tail_has_current_cache(&native.cache)
                                .then(|| cache_evaluation(&surface.geometry, u, v))
                                .flatten(),
                        ),
                    }
                } else {
                    direct_evaluation(&surface.geometry, u, v, budget, normal)
                }
            }
            Some(definition @ ProceduralSurfaceDefinition::RollingBallJet(_)) => {
                point_evaluation(rolling_ball_jet_point(definition, u, v))
            }
            Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
                evaluate(index, support, u, v, visiting, next_depth, budget, normal)
            }
            Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
                let mut evaluation =
                    evaluate(index, source, u, v, visiting, next_depth, budget, normal)?;
                if evaluation.resource.is_some() {
                    return Some(evaluation);
                }
                if normal {
                    let placed_normal = model_surface_jet_by_id(index, source, u, v, budget)
                        .map_err(|failure| failure.map(|_| ()))
                        .and_then(|jet| {
                            let [du, dv] = placed_vectors(*transform, jet.first?)?;
                            du.get()
                                .cross(dv.get())
                                .unit()
                                .ok_or(EvaluationFailure::NoValue)
                        });
                    evaluation.oriented_normal = placed_normal.or_else(|placed_failure| {
                        evaluation
                            .oriented_normal
                            .and_then(|normal| {
                                transform
                                    .apply_normal(normal)
                                    .ok_or(EvaluationFailure::NonFinite(()))
                            })
                            .and_then(|normal| {
                                Ok(scale_vector(
                                    *normal.as_raw(),
                                    transform.orientation().ok_or(EvaluationFailure::NoValue)?,
                                ))
                            })
                            .map_err(|evaluation_failure| match placed_failure {
                                EvaluationFailure::NonFinite(()) => placed_failure,
                                EvaluationFailure::NoValue => evaluation_failure,
                                EvaluationFailure::ResourceLimit(limit) => {
                                    EvaluationFailure::ResourceLimit(limit)
                                }
                            })
                    });
                }
                evaluation.point = match evaluation.point {
                    Ok(point) => transform.apply_point_reaching(point.get()),
                    // The placement of a point outside the finite range stays
                    // outside it, whatever point the placement reaches.
                    Err(point) => Err(placed_reach(*transform, point)),
                };
                Some(evaluation)
            }
            Some(ProceduralSurfaceDefinition::Subset(definition_payload)) => {
                let support = definition_payload.support();
                let parameter_ranges = definition_payload
                    .parameter_ranges()
                    .map(crate::geometry::DirectedParameterRange::finite_endpoints);
                let u_sense = definition_payload.u_sense();
                let v_sense = definition_payload.v_sense();
                {
                    let (support_u, support_v, u_derivative, v_derivative) =
                        subset_support_parameters_with_derivatives(
                            u,
                            v,
                            parameter_ranges,
                            *u_sense,
                            *v_sense,
                        )?;
                    let mut evaluation = evaluate(
                        index, support, support_u, support_v, visiting, next_depth, budget, normal,
                    )?;
                    if u_derivative * v_derivative < 0.0 {
                        evaluation.oriented_normal = evaluation
                            .oriented_normal
                            .map(|normal| scale_vector(normal, -1.0));
                    }
                    Some(evaluation)
                }
            }
            Some(ProceduralSurfaceDefinition::ParallelOffset(definition_payload)) => {
                let support = definition_payload.support();
                let distance = definition_payload.distance();
                {
                    let support =
                        evaluate(index, support, u, v, visiting, next_depth, budget, true)?;
                    if support.resource.is_some() {
                        return Some(support);
                    }
                    match support.oriented_normal {
                        Ok(normal) => Some(SurfaceEvaluation {
                            point: evaluated(offset(
                                reached(support.point),
                                &[(distance.get(), normal)],
                            )),
                            oriented_normal: Ok(normal),
                            resource: None,
                        }),
                        Err(failure) => offset_without_normal(&support, failure),
                    }
                }
            }
            Some(ProceduralSurfaceDefinition::Offset(definition_payload)) => {
                let support = definition_payload.support();
                let distance = definition_payload.distance();
                let linear_extension = definition_payload.linear_support_extension();
                {
                    let support = linear_extension
                        .then(|| linear_nurbs_support_extension(index, support, u, v, budget))
                        .flatten()
                        .or_else(|| {
                            evaluate(index, support, u, v, visiting, next_depth, budget, true)
                        })?;
                    if support.resource.is_some() {
                        return Some(support);
                    }
                    match support.oriented_normal {
                        Ok(normal) => Some(SurfaceEvaluation {
                            point: evaluated(offset(
                                reached(support.point),
                                &[(distance.get(), normal)],
                            )),
                            oriented_normal: Ok(normal),
                            resource: None,
                        }),
                        Err(failure) => offset_without_normal(&support, failure),
                    }
                }
            }
            _ if procedural.is_some() => {
                // A non-finite point enters the evaluation as a finite one
                // does.
                point_evaluation(model_surface_point_with_budget(
                    index.ir(),
                    &surface.geometry,
                    u,
                    v,
                    budget,
                ))
            }
            _ => direct_evaluation(&surface.geometry, u, v, budget, normal),
        };
        result.map(|evaluation| {
            let limit = evaluation.resource.or(match evaluation.oriented_normal {
                Err(EvaluationFailure::ResourceLimit(limit)) => Some(limit),
                _ => None,
            });
            limit.map_or(evaluation, resource)
        })
    }

    let evaluation = evaluate(
        index,
        surface,
        u,
        v,
        &mut [None; MAX_MODEL_EVALUATION_DEPTH],
        0,
        budget,
        false,
    )
    .ok_or(EvaluationFailure::NoValue)?;
    if let Some(limit) = evaluation.resource {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    evaluation.point.map_err(EvaluationFailure::NonFinite)
}
