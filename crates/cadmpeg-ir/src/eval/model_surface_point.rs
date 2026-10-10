// SPDX-License-Identifier: Apache-2.0
//! Model surface point evaluation across stored and procedural carriers.

use super::admission;
use super::cacheless_constant_rolling_ball_first_order;
use super::cacheless_constant_rolling_ball_point;
use super::cacheless_law_sweep_point;
use super::cacheless_law_sweep_first_order;
use super::cacheless_variable_blend_point;
use super::model_axis_revolution_point;
use super::model_axis_revolution_jet;
use super::model_linear_sweep_point;
use super::model_linear_sweep_jet;
use super::model_native_extrusion_point;
use super::model_native_extrusion_jet;
use super::model_native_revolution_point;
use super::model_native_revolution_jet;
use super::model_ruled_surface_point;
use super::model_ruled_surface_jet;
use super::model_sum_surface_point;
use super::model_sum_surface_jet;
use super::surface_request::{model_jet, SurfaceRequest};
use super::model_surface_point;
use super::offset;
use super::placed_reach;
use super::placed_vectors;
use super::record_u_interval;
use super::revision_surface_tail_has_current_cache;
use super::rolling_ball_jet_point_admitted;
use super::scale_vector;
use super::subset_support_parameters_with_derivatives;
use super::surface_first_order;
use super::sweep_has_current_cache;
use super::unit_cross_direction;
use super::variable_blend_has_current_cache;
use super::EvaluationFailure;
use super::ModelEvaluationDepthGuard;
use super::ModelEvaluationIdentity;
use super::UNREACHED_POINT;
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point3, Vector3};
use cadmpeg_core::decode::ResourceLimit;

pub(super) fn model_surface_point_by_id_inner(
    admission: admission::EvaluationAdmission<'_, '_>,
    index: &crate::index::ModelIndex<'_>,
    surface: &crate::ids::SurfaceId,
    u: f64,
    v: f64,
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

    /// A stored cache's point and, where the reader needs it, unit normal.
    /// The normal is that of the cache's first partials.
    fn cache_evaluation(
        admission: admission::EvaluationAdmission<'_, '_>,
        geometry: &SurfaceGeometry,
        u: f64,
        v: f64,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        if !normal {
            return point_evaluation(crate::eval::decode::surface_point(admission, geometry, u, v));
        }
        let point = match crate::eval::decode::surface_point(admission, geometry, u, v) {
            Ok(point) => Ok(point),
            Err(EvaluationFailure::NonFinite(point)) => Err(point),
            Err(EvaluationFailure::NoValue) => return None,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
        };
        let oriented_normal = surface_first_order(admission, geometry, u, v)
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
        admission: admission::EvaluationAdmission<'_, '_>,
        geometry: &SurfaceGeometry,
        u: f64,
        v: f64,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        if !normal {
            return point_evaluation(crate::eval::decode::surface_point(
                admission, geometry, u, v,
            ));
        }
        let reversed = matches!(
            geometry,
            SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) if nurbs.normal_reversed()
        );
        first_evaluation(surface_first_order(admission, geometry, u, v), reversed)
    }

    /// Keep the actual selected point separate from its first-order normal.
    /// A derivative refusal remains the original outer resource error; other
    /// derivative failures leave this point available to readers of the point.
    fn first_evaluation(
        order: Result<super::SurfaceFirstOrder, EvaluationFailure<Point3>>,
        reversed: bool,
    ) -> Option<SurfaceEvaluation> {
        let order = match order {
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
        admission: admission::EvaluationAdmission<'_, '_>,
        index: &crate::index::ModelIndex<'_>,
        support: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        let support = match index.surfaces(support.as_str(), admission) {
            Ok(Some(support)) => support,
            Ok(None) => return None,
            Err(limit) => return Some(resource(limit)),
        };
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
        let order = match surface_first_order(admission, &support.geometry, boundary_u, boundary_v)
        {
            Ok(order) => order,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return None,
        };
        let partials = match order.partials() {
            Ok(partials) => partials,
            Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
            Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(_)) => return None,
        };
        let oriented_normal = if normal {
            match oriented_normal(Ok([partials.du, partials.dv]), nurbs.normal_reversed()) {
                Ok(normal) => Ok(normal),
                Err(EvaluationFailure::ResourceLimit(limit)) => return Some(resource(limit)),
                Err(EvaluationFailure::NoValue | EvaluationFailure::NonFinite(())) => return None,
            }
        } else { Err(EvaluationFailure::NoValue) };
        let partials = partials.into_raw();
        let du = u - boundary_u;
        let dv = v - boundary_v;
        Some(SurfaceEvaluation {
            point: evaluated(Point3::new(
                partials.point.x + du * partials.du.x + dv * partials.dv.x,
                partials.point.y + du * partials.du.y + dv * partials.dv.y,
                partials.point.z + du * partials.du.z + dv * partials.dv.z,
            )),
            oriented_normal,
            resource: None,
        })
    }

    /// The evaluation of `surface_id` at `(u, v)`, with the oriented normal
    /// where `normal` is set: an offset reads its support's normal, and a
    /// placement or subset passes its reader's need on to its support.
    fn evaluate(
        admission: admission::EvaluationAdmission<'_, '_>,
        index: &crate::index::ModelIndex<'_>,
        surface_id: &crate::ids::SurfaceId,
        u: f64,
        v: f64,
        normal: bool,
    ) -> Option<SurfaceEvaluation> {
        let budget = admission.work_slice();
        let depth_guard = match ModelEvaluationDepthGuard::enter(budget) {
            Ok(guard) => guard,
            Err(limit) => return Some(resource(limit)),
        };
        if let Err(failure) = admission.model_step() {
            return match failure {
                EvaluationFailure::ResourceLimit(limit) => Some(resource(limit)),
                EvaluationFailure::NoValue | EvaluationFailure::NonFinite(()) => None,
            };
        }
        let surface = match index.surfaces(surface_id.as_str(), admission) {
            Ok(Some(surface)) => surface,
            Ok(None) => return None,
            Err(limit) => return Some(resource(limit)),
        };
        if !depth_guard.bind(
            ModelEvaluationIdentity::Surface(std::ptr::from_ref(surface)),
            admission,
        ) {
            return None;
        }
        let procedural = match index.procedural_surface_for_surface(surface_id.as_str(), admission)
        {
            Ok(value) => value,
            Err(limit) => return Some(resource(limit)),
        };
        let carrier_interval =
            procedural.and_then(|procedural| record_u_interval(procedural.record_bounds()));
        let result = match procedural.map(crate::geometry::ProceduralSurface::definition) {
            Some(ProceduralSurfaceDefinition::AxisRevolution(definition_payload)) => {
                if normal {
                    first_evaluation(model_axis_revolution_jet(
                        admission, index, definition_payload.directrix(),
                        definition_payload.axis_origin().get(), definition_payload.axis_direction(),
                        u, v, SurfaceRequest::First,
                    ).map(|requested| requested.jet.first_order()), false)
                } else {
                    point_evaluation(model_axis_revolution_point(
                        admission,
                        index,
                        definition_payload.directrix(),
                        definition_payload.axis_origin().get(),
                        definition_payload.axis_direction(),
                        u,
                        v,
                    ))
                }
            }
            Some(ProceduralSurfaceDefinition::Extrusion(definition_payload)) => {
                if normal {
                    first_evaluation(
                        model_native_extrusion_jet(
                            admission, index, definition_payload, carrier_interval, u, v,
                            SurfaceRequest::First,
                        ).map(|requested| requested.jet.first_order()),
                        false,
                    )
                } else {
                    point_evaluation(model_native_extrusion_point(
                        admission,
                        index,
                        definition_payload,
                        carrier_interval,
                        u,
                        v,
                    ))
                }
            }
            Some(ProceduralSurfaceDefinition::LinearSweep(definition_payload)) => {
                if normal {
                    first_evaluation(model_linear_sweep_jet(
                        admission, index, definition_payload, u, v, SurfaceRequest::First,
                    ).map(|requested| requested.jet.first_order()), false)
                } else {
                    point_evaluation(model_linear_sweep_point(admission, index, definition_payload, u, v))
                }
            }
            Some(ProceduralSurfaceDefinition::Revolution(definition_payload)) => {
                if normal {
                    first_evaluation(
                        model_native_revolution_jet(
                            admission, index, definition_payload, carrier_interval, u, v,
                            SurfaceRequest::First,
                        ).map(|requested| requested.jet.first_order()),
                        false,
                    )
                } else {
                    point_evaluation(model_native_revolution_point(
                        admission,
                        index,
                        definition_payload,
                        carrier_interval,
                        u,
                        v,
                    ))
                }
            }
            Some(ProceduralSurfaceDefinition::Ruled { first, second, .. }) => {
                if normal {
                    first_evaluation(model_ruled_surface_jet(
                        admission, index, first, second, u, v, SurfaceRequest::First,
                    ).map(|requested| requested.jet.first_order()), false)
                } else {
                    point_evaluation(model_ruled_surface_point(admission, index, first, second, u, v))
                }
            }
            Some(ProceduralSurfaceDefinition::Sum(definition_payload)) => {
                if normal {
                    first_evaluation(model_sum_surface_jet(
                        admission, index, definition_payload, u, v, SurfaceRequest::First,
                    ).map(|requested| requested.jet.first_order()), false)
                } else {
                    point_evaluation(model_sum_surface_point(admission, index, definition_payload, u, v))
                }
            }
            Some(ProceduralSurfaceDefinition::Sweep(definition_payload)) => {
                if let Some(construction) = definition_payload.native() {
                    let profile = definition_payload.profile();
                    let spine = definition_payload.spine();

                    if normal {
                        return match cacheless_law_sweep_first_order(
                            admission, index, profile, spine, construction, u, v,
                        ) {
                            Ok(order) => first_evaluation(Ok(order), false),
                            Err(EvaluationFailure::ResourceLimit(limit)) => Some(resource(limit)),
                            Err(failure) => cache_fallback(
                                failure,
                                sweep_has_current_cache(construction)
                                    .then(|| cache_evaluation(admission, &surface.geometry, u, v, true))
                                    .flatten(),
                            ),
                        };
                    }
                    match cacheless_law_sweep_point(
                        admission,
                        index,
                        profile,
                        spine,
                        construction,
                        u,
                        v,
                    ) {
                        Err(EvaluationFailure::ResourceLimit(limit)) => Some(resource(limit)),
                        Ok(point) => Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal: Err(EvaluationFailure::NoValue),
                            resource: None,
                        }),
                        Err(failure) => cache_fallback(
                            failure,
                            sweep_has_current_cache(construction)
                                .then(|| cache_evaluation(admission, &surface.geometry, u, v, normal))
                                .flatten(),
                        ),
                    }
                } else {
                    direct_evaluation(admission, &surface.geometry, u, v, normal)
                }
            }
            Some(ProceduralSurfaceDefinition::VariableBlend(definition_payload)) => {
                let construction = definition_payload.construction();

                match cacheless_variable_blend_point(admission, index, definition_payload, u, v) {
                    Err(EvaluationFailure::ResourceLimit(limit)) => Some(resource(limit)),
                    Ok(point) => Some(SurfaceEvaluation {
                        point: evaluated(point),
                        oriented_normal: Err(EvaluationFailure::NoValue),
                        resource: None,
                    }),
                    Err(failure) => cache_fallback(
                        failure,
                        variable_blend_has_current_cache(construction)
                            .then(|| cache_evaluation(admission, &surface.geometry, u, v, normal))
                            .flatten(),
                    ),
                }
            }
            Some(ProceduralSurfaceDefinition::Blend(definition_payload)) => {
                if let Some(native) = definition_payload.native() {
                    match cacheless_constant_rolling_ball_point(
                        admission,
                        index,
                        definition_payload,
                        u,
                        v,
                    ) {
                        Err(EvaluationFailure::ResourceLimit(limit)) => Some(resource(limit)),
                        Ok(point) => Some(SurfaceEvaluation {
                            point: evaluated(point),
                            oriented_normal: if normal {
                                cacheless_constant_rolling_ball_first_order(
                                    admission,
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
                                .then(|| cache_evaluation(admission, &surface.geometry, u, v, normal))
                                .flatten(),
                        ),
                    }
                } else {
                    direct_evaluation(admission, &surface.geometry, u, v, normal)
                }
            }
            Some(definition @ ProceduralSurfaceDefinition::RollingBallJet(_)) => {
                point_evaluation(rolling_ball_jet_point_admitted(admission, definition, u, v))
            }
            Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => {
                evaluate(admission, index, support, u, v, normal)
            }
            Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
                let mut evaluation = evaluate(admission, index, source, u, v, normal)?;
                if evaluation.resource.is_some() {
                    return Some(evaluation);
                }
                if normal {
                    let placed_normal = model_jet(admission, index, source, u, v, SurfaceRequest::First)
                        .map_err(|failure| failure.map(|_| ()))
                        .and_then(|jet| {
                            let [du, dv] = placed_vectors(*transform, jet.first?)?;
                            du.get()
                                .cross(dv.get())
                                .unit()
                                .ok_or(EvaluationFailure::NoValue)
                        });
                    if let Err(EvaluationFailure::ResourceLimit(limit)) = placed_normal {
                        return Some(resource(limit));
                    }
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
                    let mut evaluation =
                        evaluate(admission, index, support, support_u, support_v, normal)?;
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
                    let support = evaluate(admission, index, support, u, v, normal || distance.get() != 0.0)?;
                    if distance.get() == 0.0 { return Some(support); }
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
                    let read_normal = normal || distance.get() != 0.0;
                    let support = linear_extension
                        .then(|| linear_nurbs_support_extension(admission, index, support, u, v, read_normal))
                        .flatten()
                        .or_else(|| evaluate(admission, index, support, u, v, read_normal))?;
                    if distance.get() == 0.0 { return Some(support); }
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
                point_evaluation(model_surface_point(
                    admission,
                    index.ir(),
                    &surface.geometry,
                    u,
                    v,
                ))
            }
            _ => direct_evaluation(admission, &surface.geometry, u, v, normal),
        };
        result.map(|evaluation| {
            let limit = evaluation.resource.or(match evaluation.oriented_normal {
                Err(EvaluationFailure::ResourceLimit(limit)) => Some(limit),
                _ => None,
            });
            limit.map_or(evaluation, resource)
        })
    }

    let evaluation =
        evaluate(admission, index, surface, u, v, false).ok_or(EvaluationFailure::NoValue)?;
    if let Some(limit) = evaluation.resource {
        return Err(EvaluationFailure::ResourceLimit(limit));
    }
    evaluation.point.map_err(EvaluationFailure::NonFinite)
}

#[cfg(test)]
mod tests;
