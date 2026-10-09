// SPDX-License-Identifier: Apache-2.0
//! Requested derivative order through surface carrier mappings.

pub(super) mod differentials;

use super::admission::EvaluationAdmission;
use super::depth::{ModelEvaluationDepthGuard, ModelEvaluationIdentity};
use super::{decode, EvaluationFailure, SurfaceJet};
use crate::features::FiniteVector3;
use crate::geometry::{ProceduralSurfaceDefinition, SurfaceGeometry};
use crate::ids::SurfaceId;
use crate::index::ModelIndex;
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::transform::Transform;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SurfaceRequest {
    First,
    Second,
    Third,
    // An offset's requested third needs fourth support partials. This order
    // has no implemented owner; keep the available lower orders separately.
    Fourth,
}

impl SurfaceRequest {
    fn support_order(self) -> Self {
        match self {
            Self::First => Self::Second,
            Self::Second => Self::Third,
            Self::Third | Self::Fourth => Self::Fourth,
        }
    }

    pub(super) fn needs_second(self) -> bool {
        !matches!(self, Self::First)
    }

    pub(super) fn needs_third(self) -> bool {
        matches!(self, Self::Third | Self::Fourth)
    }
}

pub(super) struct RequestedJet {
    pub(super) jet: SurfaceJet,
    pub(super) higher: HigherPartials,
}

/// A stored affine chart has constant first partials and zero higher partials.
/// This state comes only from a Plane chart and affine placement/parameter
/// reversal or constant normal offset of that state; sampled zeros cannot form it.
#[derive(Clone, Copy)]
pub(super) enum HigherPartials {
    Affine,
    Third(Result<[FiniteVector3; 4], EvaluationFailure<()>>),
}

impl HigherPartials {
    pub(super) fn third(self) -> Result<[FiniteVector3; 4], EvaluationFailure<()>> {
        match self {
            Self::Affine => Ok([FiniteVector3::ZERO; 4]),
            Self::Third(result) => result,
        }
    }

    pub(super) fn placed(self, transform: Transform) -> Self {
        match self {
            Self::Affine => Self::Affine,
            Self::Third(result) => Self::Third(result.and_then(|lanes| super::placed_vectors(transform, lanes))),
        }
    }
}

// Every recipe borrows a carrier or a stack-local placement node. No recipe
// reference escapes the continuation; no model, parameter plan or Box is copied.
enum Source<'a> {
    Stored(&'a SurfaceGeometry, f64, f64),
    Procedural(&'a ProceduralSurfaceDefinition, Option<[FiniteReal; 2]>, f64, f64),
    Replica(&'a Mapping<'a>, Transform, Option<f64>),
}

struct Mapping<'a> {
    source: Source<'a>,
    distance: Result<f64, EvaluationFailure<Point3>>,
    reversed: [bool; 2],
    orientation: Result<f64, EvaluationFailure<Point3>>,
}

impl Mapping<'_> {
    fn evaluate(
        &self,
        admission: EvaluationAdmission<'_, '_>,
        index: &ModelIndex<'_>,
        request: SurfaceRequest,
    ) -> Result<RequestedJet, EvaluationFailure<Point3>> {
        // An invalid deferred orientation must first reach its source and
        // placement, as the original Replica arm does. Do not ask for an
        // unneeded third when that segment cannot form a distance.
        let source_order = match self.distance {
            Ok(distance) if distance != 0.0 => request.support_order(),
            Err(_) => SurfaceRequest::First,
            Ok(_) => request,
        };
        let source = match &self.source {
            Source::Stored(geometry, u, v) => {
                let scratch = decode::Scratch::new(admission);
                let result = geometry.solved().ok_or(EvaluationFailure::NoValue)
                    .and_then(|geometry| super::surface_requested_jet_solved(&scratch, geometry, *u, *v, source_order));
                scratch.settle(result)
            }
            Source::Procedural(definition, interval, u, v) => {
                match definition {
                    ProceduralSurfaceDefinition::AxisRevolution(payload) => super::model_axis_revolution_jet(
                        admission, index, payload.directrix(), payload.axis_origin().get(), payload.axis_direction(), *u, *v,
                        source_order,
                    ),
                    ProceduralSurfaceDefinition::Extrusion(payload) => super::model_native_extrusion_jet(admission, index, payload, *interval, *u, *v, source_order),
                    ProceduralSurfaceDefinition::LinearSweep(payload) => super::model_linear_sweep_jet(admission, index, payload, *u, *v, source_order),
                    ProceduralSurfaceDefinition::Revolution(payload) => super::model_native_revolution_jet(admission, index, payload, *interval, *u, *v, source_order),
                    ProceduralSurfaceDefinition::Ruled { first, second, .. } => super::model_ruled_surface_jet(admission, index, first, second, *u, *v, source_order),
                    ProceduralSurfaceDefinition::Sum(payload) => super::model_sum_surface_jet(admission, index, payload, *u, *v, source_order),
                    _ => Err(EvaluationFailure::NoValue),
                }
            }
            Source::Replica(source, transform, orientation) => {
                // The borrowed recipe follows one actual placement node here.
                // Carrier lookup was admitted during construction; this second
                // pointer walk is separate input-dependent work.
                admission.independent_cost(Some(1))?;
                admission.work(1, "IR surface placement source traversal")
                    .map_err(EvaluationFailure::ResourceLimit)?;
                // A singular placement cannot publish an oriented chart. Its
                // source point/first still run before that error; an offset's
                // missing second or third does not affect this barrier.
                let source_order = if orientation.is_none() { SurfaceRequest::First } else { source_order };
                let source = source.evaluate(admission, index, source_order)?;
                let jet = super::placed_jet(*transform, Ok(source.jet))?;
                let higher = source.higher.placed(*transform);
                // The pure orientation result is deferred until the existing
                // source-offset and placed-point/partial barriers have run.
                orientation.ok_or(EvaluationFailure::NoValue)?;
                Ok(RequestedJet { jet, higher })
            }
        }?;
        let distance = self.distance?;
        if distance == 0.0 {
            return Ok(source);
        }
        offset(source, distance, request)
    }
}

pub(super) fn model_jet(
    admission: EvaluationAdmission<'_, '_>,
    index: &ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    request: SurfaceRequest,
) -> Result<SurfaceJet, EvaluationFailure<Point3>> {
    with_mapping(admission, index, surface, u, v, &mut |mapping| {
        let result = mapping.evaluate(admission, index, request)?;
        let [u_reversed, v_reversed] = mapping.reversed;
        let reverse = |vector: FiniteVector3, reversed| if reversed { vector.negated() } else { vector };
        Ok(SurfaceJet {
            point: result.jet.point,
            first: result.jet.first.map(|[du, dv]| [reverse(du, u_reversed), reverse(dv, v_reversed)]),
            second: result.jet.second.map(|[duu, duv, dvv]| [duu, reverse(duv, u_reversed != v_reversed), dvv]),
        })
    })
}

fn with_mapping<R>(
    admission: EvaluationAdmission<'_, '_>,
    index: &ModelIndex<'_>,
    surface: &SurfaceId,
    u: f64,
    v: f64,
    consume: &mut dyn for<'node> FnMut(Mapping<'node>) -> Result<R, EvaluationFailure<Point3>>,
) -> Result<R, EvaluationFailure<Point3>> {
    let depth = ModelEvaluationDepthGuard::enter(admission.work_slice()).map_err(EvaluationFailure::ResourceLimit)?;
    admission.model_step()?;
    let carrier = index.surfaces(surface.as_str(), admission).map_err(EvaluationFailure::ResourceLimit)?.ok_or(EvaluationFailure::NoValue)?;
    if !depth.bind(ModelEvaluationIdentity::Surface(std::ptr::from_ref(carrier)), admission) {
        return Err(EvaluationFailure::NoValue);
    }
    let procedural = index.procedural_surface_for_surface(surface.as_str(), admission).map_err(EvaluationFailure::ResourceLimit)?;
    let interval = procedural.and_then(|procedural| super::record_u_interval(procedural.record_bounds()));
    let definition = procedural.map(crate::geometry::ProceduralSurface::definition);
    match definition {
        Some(ProceduralSurfaceDefinition::CurveBounded { support, .. }) => with_mapping(admission, index, support, u, v, consume),
        Some(ProceduralSurfaceDefinition::Replica { source, transform }) => {
            with_mapping(admission, index, source, u, v, &mut |source| {
                let orientation = transform.orientation();
                let combined = source.orientation.and_then(|sign| Ok(sign * orientation.ok_or(EvaluationFailure::NoValue)?));
                consume(Mapping {
                    source: Source::Replica(&source, *transform, orientation),
                    distance: Ok(0.0),
                    reversed: source.reversed,
                    orientation: combined,
                })
            })
        }
        Some(ProceduralSurfaceDefinition::Subset(payload)) => {
            let ranges = payload.parameter_ranges().map(crate::geometry::DirectedParameterRange::finite_endpoints);
            let (support_u, support_v, u_derivative, v_derivative) = super::subset_support_parameters_with_derivatives(u, v, ranges, *payload.u_sense(), *payload.v_sense()).ok_or(EvaluationFailure::NoValue)?;
            with_mapping(admission, index, payload.support(), support_u, support_v, &mut |mut support| {
                support.reversed[0] ^= u_derivative < 0.0;
                support.reversed[1] ^= v_derivative < 0.0;
                support.orientation = support.orientation.map(|sign| sign * u_derivative * v_derivative);
                consume(support)
            })
        }
        Some(ProceduralSurfaceDefinition::ParallelOffset(payload)) => {
            with_mapping(admission, index, payload.support(), u, v, &mut |mut support| {
                support.distance = support.distance.and_then(|distance| Ok(distance + payload.distance().get() * support.orientation?));
                consume(support)
            })
        }
        Some(ProceduralSurfaceDefinition::Offset(payload)) => {
            with_mapping(admission, index, payload.support(), u, v, &mut |mut support| {
                support.distance = support.distance.and_then(|distance| Ok(distance + payload.distance().get() * support.orientation?));
                consume(support)
            })
        }
        Some(definition @ (ProceduralSurfaceDefinition::AxisRevolution(_)
            | ProceduralSurfaceDefinition::Extrusion(_)
            | ProceduralSurfaceDefinition::LinearSweep(_)
            | ProceduralSurfaceDefinition::Revolution(_)
            | ProceduralSurfaceDefinition::Ruled { .. }
            | ProceduralSurfaceDefinition::Sum(_))) => consume(Mapping {
                source: Source::Procedural(definition, interval, u, v), distance: Ok(0.0), reversed: [false, false], orientation: Ok(1.0),
            }),
        _ => {
            // Match the directly stored point owner's oriented normal. The
            // flag changes offset direction, not the chart derivatives.
            let reversed = matches!(
                &carrier.geometry,
                SurfaceGeometry::Solved(crate::geometry::SolvedSurfaceGeometry::Nurbs(nurbs))
                    if nurbs.normal_reversed()
            );
            consume(Mapping {
                source: Source::Stored(&carrier.geometry, u, v), distance: Ok(0.0),
                reversed: [false, false], orientation: Ok(if reversed { -1.0 } else { 1.0 }),
            })
        },
    }
}

fn offset(
    source: RequestedJet,
    distance: f64,
    request: SurfaceRequest,
) -> Result<RequestedJet, EvaluationFailure<Point3>> {
    let base = source.jet;
    let unreached = EvaluationFailure::NonFinite(super::UNREACHED_POINT);
    if !distance.is_finite() { return Err(unreached); }
    let [du, dv] = FiniteVector3::raw_array(base.first.map_err(|failure| failure.map(|()| super::UNREACHED_POINT))?);
    let normal_vector = du.cross(dv);
    let magnitude = normal_vector.norm();
    if !magnitude.is_finite() { return Err(unreached); }
    if magnitude == 0.0 { return Err(EvaluationFailure::NoValue); }
    let normal = Vector3::new(normal_vector.x / magnitude, normal_vector.y / magnitude, normal_vector.z / magnitude);
    let original_point = base.point.get();
    let point = super::admit_point(Point3::new(
        original_point.x + distance * normal.x,
        original_point.y + distance * normal.y,
        original_point.z + distance * normal.z,
    ))?;
    let unit_derivative = |derivative: Vector3| {
        let component = normal.x * derivative.x + normal.y * derivative.y + normal.z * derivative.z;
        Vector3::new((derivative.x - component * normal.x) / magnitude, (derivative.y - component * normal.y) / magnitude, (derivative.z - component * normal.z) / magnitude)
    };
    let first = base.second.and_then(|second| {
        let [duu, duv, dvv] = FiniteVector3::raw_array(second);
        let normal_u = unit_derivative(super::vector_sum(&[(1.0, duu.cross(dv)), (1.0, du.cross(duv))]));
        let normal_v = unit_derivative(super::vector_sum(&[(1.0, duv.cross(dv)), (1.0, du.cross(dvv))]));
        super::admit_lanes([
            Vector3::new(du.x + distance * normal_u.x, du.y + distance * normal_u.y, du.z + distance * normal_u.z),
            Vector3::new(dv.x + distance * normal_v.x, dv.y + distance * normal_v.y, dv.z + distance * normal_v.z),
        ])
    });
    let second = if matches!(source.higher, HigherPartials::Affine) {
        // An affine chart has a constant normal; a constant offset preserves
        // its derivatives at every order, including after a Replica.
        base.second
    } else if request == SurfaceRequest::First {
        Err(EvaluationFailure::NoValue)
    } else {
        differentials::offset_second(base, source.higher.third(), distance, normal, magnitude)
    };
    Ok(RequestedJet { jet: SurfaceJet { point, first, second }, higher: match source.higher {
        HigherPartials::Affine => HigherPartials::Affine,
        HigherPartials::Third(_) => HigherPartials::Third(Err(EvaluationFailure::NoValue)),
    } })
}

#[cfg(test)]
mod tests;
