// SPDX-License-Identifier: Apache-2.0
//! Tensor-product local NURBS surface evaluation and rational partials.

use std::borrow::Cow;

use super::{basis, decode, periodic_parameter, EvaluationFailure, SurfaceFirstOrder, SurfaceJet};
use super::rational::{finite_lanes, Homogeneous};
use super::surface_request::{HigherPartials, RequestedJet, SurfaceRequest};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::{NurbsPoleGrid, NurbsSurface};
use crate::math::Point3;
use crate::scalar::{FiniteReal, NonZeroReal};

mod polynomial_fourth;

/// A tensor-product NURBS surface at a parameter: its spans, its bases and
/// its homogeneous base sum, with its finite point.
pub(super) struct NurbsSurfaceLocal<'a> {
    surface: &'a NurbsSurface,
    degrees: [usize; 2],
    spans: [usize; 2],
    parameters: [f64; 2],
    bases: [decode::SupportValues<f64>; 2],
    base: Homogeneous,
    pub(super) point: [FiniteReal; 3],
}

/// The first partials of a NURBS surface at a parameter: the derivative
/// bases, the homogeneous derivative sums and the finite lanes.
pub(super) struct NurbsSurfaceFirstPartials {
    bases: [Vec<f64>; 2],
    sums: [Homogeneous; 2],
    pub(super) lanes: [[FiniteReal; 3]; 2],
}

/// The second derivative bases, homogeneous sums and projected lanes.
/// The requested third order reuses all of this actual local state.
pub(super) struct NurbsSurfaceSecondPartials {
    bases: [Cow<'static, [f64]>; 2],
    sums: [Homogeneous; 3],
    pub(super) lanes: [[FiniteReal; 3]; 3],
}

/// Completed third bases and homogeneous sums, with their projected lanes.
/// Fourth partials reuse this state without evaluating an earlier order again.
pub(super) struct NurbsSurfaceThirdPartials {
    bases: [Cow<'static, [f64]>; 2],
    sums: [Homogeneous; 4],
    pub(super) lanes: [[FiniteReal; 3]; 4],
}

/// The homogeneous sum of a NURBS surface's poles local to `spans`, blended
/// by `u_values` along `u` and `v_values` along `v`. A missing pole and a
/// value that is not finite leave no sum.
fn nurbs_local_sum(
    scratch: &decode::Scratch<'_, '_>,
    surface: &NurbsSurface,
    [u_degree, v_degree]: [usize; 2],
    [u_span, v_span]: [usize; 2],
    u_values: &[f64],
    v_values: &[f64],
) -> Option<Homogeneous> {
    scratch.admit(Homogeneous::sum(
        scratch,
        u_values
            .iter()
            .copied()
            .enumerate()
            .flat_map(|(i, u_value)| {
                v_values
                    .iter()
                    .copied()
                    .enumerate()
                    .map(move |(j, v_value)| {
                        let (pole_u, pole_v) = (u_span - u_degree + i, v_span - v_degree + j);
                        Some((
                            [u_value, v_value],
                            surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                            surface.pole(pole_u, pole_v)?,
                        ))
                    })
            }),
    ))?
}

impl NurbsSurfaceLocal<'_> {
    /// The homogeneous sum of the local poles blended by `u_values` along
    /// `u` and `v_values` along `v`.
    fn sum(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        u_values: &[f64],
        v_values: &[f64],
    ) -> Option<Homogeneous> {
        nurbs_local_sum(
            scratch,
            self.surface,
            self.degrees,
            self.spans,
            u_values,
            v_values,
        )
    }

    /// A higher homogeneous derivative sum over the exact pole window.
    fn derivative_sum(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        u_values: &[f64],
        v_values: &[f64],
    ) -> Option<Homogeneous> {
        let count = u_values.len().checked_mul(v_values.len())?;
        scratch.admit(Homogeneous::derivative_sum(scratch, (0..count).map(|local| {
            let (i, j) = (local / v_values.len(), local % v_values.len());
            let (pole_u, pole_v) = (self.spans[0] - self.degrees[0] + i, self.spans[1] - self.degrees[1] + j);
            Some((
                [u_values[i], v_values[j]],
                self.surface.weight(pole_u, pole_v).map_or(1.0, NonZeroReal::get),
                self.surface.pole(pole_u, pole_v)?,
            ))
        })))?
    }

    /// The first partials, or why they have none. At the finite point over
    /// finite knots, a derivative basis that is absent, a sum that is absent
    /// and a projection that is absent or overflows each left the finite
    /// range.
    pub(super) fn first(
        &self,
        scratch: &decode::Scratch<'_, '_>,
    ) -> Result<NurbsSurfaceFirstPartials, EvaluationFailure<()>> {
        scratch.settle((|| {
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];

            let derivative = |axis: usize| {
                basis::bspline_basis_derivative(
                    scratch,
                    knots[axis],
                    self.degrees[axis],
                    self.spans[axis],
                    self.parameters[axis],
                )
                .ok_or_else(|| scratch.failure(non_finite))
            };
            let bases = [derivative(0)?, derivative(1)?];
            let u = self
                .sum(scratch, &bases[0], &self.bases[1])
                .ok_or(non_finite)?;
            let v = self
                .sum(scratch, &self.bases[0], &bases[1])
                .ok_or(non_finite)?;
            let lane = |sum: Homogeneous| {
                finite_lanes(
                    sum.project(self.base, &[(sum, self.point)])
                        .ok_or(non_finite)?,
                )
                .map_err(|_| non_finite)
            };
            let lanes = [lane(u)?, lane(v)?];
            Ok(NurbsSurfaceFirstPartials {
                bases,
                sums: [u, v],
                lanes,
            })
        })())
    }

    /// The second partials over `first`, or why they have none, in the terms
    /// of [`Self::first`].
    pub(super) fn second(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        first: &NurbsSurfaceFirstPartials,
    ) -> Result<NurbsSurfaceSecondPartials, EvaluationFailure<()>> {
        scratch.settle((|| {
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];

            let second = |axis: usize| {
                basis::bspline_basis_second_derivative(
                    scratch,
                    knots[axis],
                    self.degrees[axis],
                    self.spans[axis],
                    self.parameters[axis],
                )
                .ok_or_else(|| scratch.failure(non_finite))
            };
            let [u_second, v_second] = [second(0)?, second(1)?];
            let [u, v] = first.sums;
            let [du, dv] = first.lanes;
            let uu = self
                .sum(scratch, &u_second, &self.bases[1])
                .ok_or(non_finite)?;
            let uv = self
                .sum(scratch, &first.bases[0], &first.bases[1])
                .ok_or(non_finite)?;
            let vv = self
                .sum(scratch, &self.bases[0], &v_second)
                .ok_or(non_finite)?;
            let lane = |sum: Homogeneous, corrections: &[(Homogeneous, [FiniteReal; 3])]| {
                finite_lanes(sum.project(self.base, corrections).ok_or(non_finite)?)
                    .map_err(|_| non_finite)
            };
            let lanes = [
                lane(uu, &[(uu, self.point), (u, du), (u, du)])?,
                lane(uv, &[(uv, self.point), (u, dv), (v, du)])?,
                lane(vv, &[(vv, self.point), (v, dv), (v, dv)])?,
            ];
            Ok(NurbsSurfaceSecondPartials {
                bases: [u_second, v_second], sums: [uu, uv, vv], lanes,
            })
        })())
    }
    /// Differentiate H=w*S three times. Polynomial homogeneous derivatives
    /// above the stored degree are zero; rational quotient corrections remain.
    pub(super) fn third(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        first: &NurbsSurfaceFirstPartials,
        second: &NurbsSurfaceSecondPartials,
    ) -> Result<NurbsSurfaceThirdPartials, EvaluationFailure<()>> {
        scratch.settle((|| {
            scratch.admission.independent_cost(nurbs_surface_third_evaluation_cost(self.degrees))?;
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];
            let third = |axis: usize| basis::bspline_basis_third_derivative(
                scratch, knots[axis], self.degrees[axis], self.spans[axis], self.parameters[axis],
            ).ok_or_else(|| scratch.failure(non_finite));
            let bases = [third(0)?, third(1)?];
            let sum = |active, u: &[f64], v: &[f64]| if active {
                self.derivative_sum(scratch, u, v).ok_or(non_finite)
            } else { Ok(Homogeneous::zero()) };
            let [u_degree, v_degree] = self.degrees;
            let uuu = sum(u_degree >= 3, &bases[0], &self.bases[1])?;
            let uuv = sum(u_degree >= 2 && v_degree >= 1, &second.bases[0], &first.bases[1])?;
            let uvv = sum(u_degree >= 1 && v_degree >= 2, &first.bases[0], &second.bases[1])?;
            let vvv = sum(v_degree >= 3, &self.bases[0], &bases[1])?;
            let [u, v] = first.sums;
            let [uu, uv, vv] = second.sums;
            let [du, dv] = first.lanes;
            let [duu, duv, dvv] = second.lanes;
            let lane = |sum: Homogeneous, corrections: &[(Homogeneous, [FiniteReal; 3])]| {
                finite_lanes(sum.project(self.base, corrections).ok_or(non_finite)?).map_err(|_| non_finite)
            };
            let lanes = [
                lane(uuu, &[(uuu, self.point), (uu, du), (uu, du), (uu, du), (u, duu), (u, duu), (u, duu)])?,
                lane(uuv, &[(uuv, self.point), (uu, dv), (uv, du), (uv, du), (u, duv), (u, duv), (v, duu)])?,
                lane(uvv, &[(uvv, self.point), (vv, du), (uv, dv), (uv, dv), (v, duv), (v, duv), (u, dvv)])?,
                lane(vvv, &[(vvv, self.point), (vv, dv), (vv, dv), (vv, dv), (v, dvv), (v, dvv), (v, dvv)])?,
            ];
            Ok(NurbsSurfaceThirdPartials { bases, sums: [uuu, uuv, uvv, vvv], lanes })
        })())
    }
}

/// Extra independent work for the actual third recurrence rows and homogeneous
/// pole traversals. Existing point/first/second independent laws are unchanged.
pub(super) fn nurbs_surface_third_evaluation_cost([u_degree, v_degree]: [usize; 2]) -> Option<usize> {
    let basis_work = |degree: usize| {
        if degree < 3 { return Some(0); }
        let base = degree - 3;
        let base_work = if base <= 1 { 0 } else {
            // Heap initialization: q+1. Cox-de Boor: first write, triangular
            // blends, then one saved value per row.
            base.checked_add(1)?.checked_add(1)?
                .checked_add(base.checked_mul(base.checked_add(1)?)?.checked_div(2)?)?
                .checked_add(base)?
        };
        base_work.checked_add(degree.checked_mul(3)?)
    };
    let supports = u_degree.checked_add(1)?.checked_mul(v_degree.checked_add(1)?)?;
    let sums = usize::from(u_degree >= 3)
        + usize::from(u_degree >= 2 && v_degree >= 1)
        + usize::from(u_degree >= 1 && v_degree >= 2)
        + usize::from(v_degree >= 3);
    let visits = if supports > 2 { supports.checked_mul(sums)? } else { 0 };
    basis_work(u_degree)?.checked_add(basis_work(v_degree)?)?.checked_add(visits)
}

/// A tensor-product NURBS surface at `(u, v)`, or why it has no finite
/// point there.
///
/// A basis that leaves the finite range reaches no coordinate, and each
/// coordinate reads NaN; a projection that overflows carries each coordinate
/// it reached. A parameter that is not finite, a knot vector or pole net that
/// states no span at the parameter, and a zero weight sum have no value.
pub(super) fn nurbs_surface_local<'a>(
    scratch: &decode::Scratch<'_, '_>,
    surface: &'a NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<NurbsSurfaceLocal<'a>, EvaluationFailure<Point3>> {
    scratch.settle(nurbs_surface_local_unsettled(scratch, surface, u_at, v_at))
}

fn nurbs_surface_local_unsettled<'a>(
    scratch: &decode::Scratch<'_, '_>,
    surface: &'a NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<NurbsSurfaceLocal<'a>, EvaluationFailure<Point3>> {
    let no_value = EvaluationFailure::NoValue;
    let unreached = EvaluationFailure::NonFinite(Point3::new(f64::NAN, f64::NAN, f64::NAN));
    let u_degree = usize::try_from(surface.u_degree()).map_err(|_| no_value)?;
    let v_degree = usize::try_from(surface.v_degree()).map_err(|_| no_value)?;
    let u_count = surface.u_count();
    let v_count = surface.v_count();
    let u_at = periodic_parameter(
        surface.u_knots(),
        u_degree,
        u_count,
        surface.u_periodic(),
        FiniteReal::new(u_at).ok_or(no_value)?,
    )
    .ok_or(no_value)?
    .get();
    let v_at = periodic_parameter(
        surface.v_knots(),
        v_degree,
        v_count,
        surface.v_periodic(),
        FiniteReal::new(v_at).ok_or(no_value)?,
    )
    .ok_or(no_value)?
    .get();
    let u_span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            surface.u_knots(),
            u_degree,
            u_count,
            u_at,
        ))
        .flatten()
        .ok_or(no_value)?;
    let v_span = scratch
        .admit(basis::bspline_span(
            scratch.admission,
            surface.v_knots(),
            v_degree,
            v_count,
            v_at,
        ))
        .flatten()
        .ok_or(no_value)?;
    // At a finite parameter over finite knots, the basis is absent or not
    // finite only where one of its terms left the finite range.
    let u_basis = basis::bspline_basis(scratch, surface.u_knots(), u_degree, u_span, u_at)
        .ok_or(unreached)?;
    let v_basis = basis::bspline_basis(scratch, surface.v_knots(), v_degree, v_span, v_at)
        .ok_or(unreached)?;
    if !basis::all_finite(scratch, &u_basis).ok_or_else(|| scratch.failure(unreached))?
        || !basis::all_finite(scratch, &v_basis).ok_or_else(|| scratch.failure(unreached))?
    {
        return Err(unreached);
    }
    let degrees = [u_degree, v_degree];
    let spans = [u_span, v_span];
    let base =
        nurbs_local_sum(scratch, surface, degrees, spans, &u_basis, &v_basis).ok_or(no_value)?;
    let point = finite_lanes(base.project(base, &[]).ok_or(no_value)?)
        .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
    Ok(NurbsSurfaceLocal {
        surface,
        degrees,
        spans,
        parameters: [u_at, v_at],
        bases: [u_basis, v_basis],
        base,
        point,
    })
}

/// A NURBS surface's point and first partials at `(u, v)`, the partials with
/// their own outcome, or why the point has none.
pub(super) fn nurbs_surface_first_order(
    scratch: &decode::Scratch<'_, '_>,
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
) -> Result<SurfaceFirstOrder, EvaluationFailure<Point3>> {
    scratch
        .admission
        .independent_cost(nurbs_surface_partials_evaluation_cost(surface))?;
    let result = (|| {
        let local = nurbs_surface_local(scratch, surface, u_at, v_at)?;
        let [x, y, z] = local.point;
        Ok(SurfaceFirstOrder {
            point: FinitePoint3::from_coordinates(x, y, z),
            first: local
                .first(scratch)
                .map(|first| first.lanes.map(finite_vector)),
        })
    })();
    scratch.settle(result)
}

/// A NURBS surface's point with its first and second partials at `(u, v)`,
/// each order with its own outcome, or why the point has none.
pub(super) fn nurbs_surface_requested_jet(
    scratch: &decode::Scratch<'_, '_>,
    surface: &NurbsSurface,
    u_at: f64,
    v_at: f64,
    request: SurfaceRequest,
) -> Result<RequestedJet, EvaluationFailure<Point3>> {
    scratch.admission.independent_cost(nurbs_surface_partials_evaluation_cost(surface))?;
    let result = (|| {
        let local = nurbs_surface_local(scratch, surface, u_at, v_at)?;
        let [x, y, z] = local.point;
        // A selected polynomial tensor chart has total degree at most p+q.
        // This stored representation proves higher zeros, not an affine
        // chart or a constant normal. Rational weights do not prove it.
        let polynomial_degree = if matches!(surface.pole_grid(), NurbsPoleGrid::Polynomial { .. }) {
            local.degrees[0].checked_add(local.degrees[1])
        } else { None };
        let first = local.first(scratch);
        let second = if request.needs_second() {
            first.as_ref().map_err(|failure| *failure)
                .and_then(|first| local.second(scratch, first))
        } else { Err(EvaluationFailure::NoValue) };
        let third_state = if request.needs_third() && !polynomial_degree.is_some_and(|degree| degree < 3) {
            first.as_ref().map_err(|failure| match *failure {
                EvaluationFailure::ResourceLimit(limit) => EvaluationFailure::ResourceLimit(limit),
                EvaluationFailure::NoValue | EvaluationFailure::NonFinite(()) => EvaluationFailure::NoValue,
            }).and_then(|first| {
                second.as_ref().map_err(|failure| match *failure {
                    EvaluationFailure::ResourceLimit(limit) => EvaluationFailure::ResourceLimit(limit),
                    EvaluationFailure::NoValue | EvaluationFailure::NonFinite(()) => EvaluationFailure::NoValue,
                })
                    .and_then(|second| local.third(scratch, first, second))
            })
        } else { Err(EvaluationFailure::NoValue) };
        let fourth = if request == SurfaceRequest::Fourth && polynomial_degree.is_some_and(|degree| degree < 4) {
            Ok([FiniteVector3::ZERO; 5])
        } else if request == SurfaceRequest::Fourth {
            match (&first, &second, &third_state) {
                (Ok(first), Ok(second), Ok(third)) => local.fourth(scratch, first, second, third),
                (Err(EvaluationFailure::ResourceLimit(limit)), _, _)
                | (_, Err(EvaluationFailure::ResourceLimit(limit)), _)
                | (_, _, Err(EvaluationFailure::ResourceLimit(limit))) => Err(EvaluationFailure::ResourceLimit(*limit)),
                _ if polynomial_degree.is_some() && (
                    matches!(first, Err(EvaluationFailure::NonFinite(())))
                    || matches!(second, Err(EvaluationFailure::NonFinite(())))
                    || matches!(third_state, Err(EvaluationFailure::NonFinite(())))
                ) => polynomial_fourth::evaluate(scratch, &local),
                // Missing lower raw state does not prove this order overflowed.
                _ => Err(EvaluationFailure::NoValue),
            }.map(|lanes| lanes.map(finite_vector))
        } else { Err(EvaluationFailure::NoValue) };
        let third = if request.needs_third() && polynomial_degree.is_some_and(|degree| degree < 3) {
            Ok([FiniteVector3::ZERO; 4])
        } else { third_state.map(|third| third.lanes.map(finite_vector)) };
        Ok(RequestedJet {
            jet: SurfaceJet {
                point: FinitePoint3::from_coordinates(x, y, z),
                first: first.map(|first| first.lanes.map(finite_vector)),
                second: second.map(|second| second.lanes.map(finite_vector)),
            },
            higher: if request == SurfaceRequest::Fourth { HigherPartials::Fourth { third, fourth } }
                else { HigherPartials::Third(third) },
        })
    })();
    scratch.settle(result)
}

/// The vector of three finite lanes.
fn finite_vector([x, y, z]: [FiniteReal; 3]) -> FiniteVector3 {
    FiniteVector3::from_components(x, y, z)
}

pub(super) fn nurbs_surface_evaluation_cost(surface: &NurbsSurface) -> Option<usize> {
    let (u_support, v_support) = nurbs_surface_support_sizes(surface)?;
    let control_work = u_support.checked_mul(v_support)?;
    control_work
        .checked_add(u_support.checked_mul(u_support)?)?
        .checked_add(v_support.checked_mul(v_support)?)
}

fn nurbs_surface_partials_evaluation_cost(surface: &NurbsSurface) -> Option<usize> {
    let (u_support, v_support) = nurbs_surface_support_sizes(surface)?;
    let control_work = u_support.checked_mul(v_support)?;
    let u_basis_work = u_support.checked_mul(u_support)?.checked_mul(3)?;
    let v_basis_work = v_support.checked_mul(v_support)?.checked_mul(3)?;
    control_work
        .checked_add(u_basis_work)?
        .checked_add(v_basis_work)
}

fn nurbs_surface_support_sizes(surface: &NurbsSurface) -> Option<(usize, usize)> {
    Some((
        usize::try_from(surface.u_degree()).ok()?.checked_add(1)?,
        usize::try_from(surface.v_degree()).ok()?.checked_add(1)?,
    ))
}

impl NurbsSurfaceLocal<'_> {
    pub(super) fn fourth(
        &self,
        scratch: &decode::Scratch<'_, '_>,
        first: &NurbsSurfaceFirstPartials,
        second: &NurbsSurfaceSecondPartials,
        third: &NurbsSurfaceThirdPartials,
    ) -> Result<[[FiniteReal; 3]; 5], EvaluationFailure<()>> {
        scratch.settle((|| {
            scratch.admission.independent_cost(fourth_evaluation_cost(self.degrees))?;
            let non_finite = EvaluationFailure::NonFinite(());
            let knots = [self.surface.u_knots(), self.surface.v_knots()];
            let basis = |axis: usize| basis::bspline_basis_fourth_derivative(
                scratch, knots[axis], self.degrees[axis], self.spans[axis], self.parameters[axis],
            ).ok_or_else(|| scratch.failure(non_finite));
            let bases = [basis(0)?, basis(1)?];
            let sum = |active, u: &[f64], v: &[f64]| if active {
                self.derivative_sum(scratch, u, v).ok_or(non_finite)
            } else { Ok(Homogeneous::zero()) };
            let [u_degree, v_degree] = self.degrees;
            let uuuu = sum(u_degree >= 4, &bases[0], &self.bases[1])?;
            let uuuv = sum(u_degree >= 3 && v_degree >= 1, &third.bases[0], &first.bases[1])?;
            let uuvv = sum(u_degree >= 2 && v_degree >= 2, &second.bases[0], &second.bases[1])?;
            let uvvv = sum(u_degree >= 1 && v_degree >= 3, &first.bases[0], &third.bases[1])?;
            let vvvv = sum(v_degree >= 4, &self.bases[0], &bases[1])?;
            let [u, v] = first.sums;
            let [uu, uv, vv] = second.sums;
            let [uuu, uuv, uvv, vvv] = third.sums;
            let [du, dv] = first.lanes;
            let [duu, duv, dvv] = second.lanes;
            let [duuu, duuv, duvv, dvvv] = third.lanes;
            // Each coefficient is the product-rule binomial multiplicity.
            // Repeated terms avoid an overflowing binary64 coefficient product.
            let lane = |sum: Homogeneous, corrections: &[(Homogeneous, [FiniteReal; 3])]| {
                finite_lanes(sum.project(self.base, corrections).ok_or(non_finite)?).map_err(|_| non_finite)
            };
            Ok([
                lane(uuuu, &[
                    (uuuu, self.point),
                    (uuu, du), (uuu, du), (uuu, du), (uuu, du),
                    (uu, duu), (uu, duu), (uu, duu), (uu, duu), (uu, duu), (uu, duu),
                    (u, duuu), (u, duuu), (u, duuu), (u, duuu),
                ])?,
                lane(uuuv, &[
                    (uuuv, self.point),
                    (uuv, du), (uuv, du), (uuv, du), (uuu, dv),
                    (uv, duu), (uv, duu), (uv, duu), (uu, duv), (uu, duv), (uu, duv),
                    (v, duuu), (u, duuv), (u, duuv), (u, duuv),
                ])?,
                lane(uuvv, &[
                    (uuvv, self.point), (uvv, du), (uvv, du), (uuv, dv), (uuv, dv),
                    (vv, duu), (uv, duv), (uv, duv), (uv, duv), (uv, duv), (uu, dvv),
                    (v, duuv), (v, duuv), (u, duvv), (u, duvv),
                ])?,
                lane(uvvv, &[
                    (uvvv, self.point), (vvv, du), (uvv, dv), (uvv, dv), (uvv, dv),
                    (vv, duv), (vv, duv), (vv, duv), (uv, dvv), (uv, dvv), (uv, dvv),
                    (v, duvv), (v, duvv), (v, duvv), (u, dvvv),
                ])?,
                lane(vvvv, &[
                    (vvvv, self.point),
                    (vvv, dv), (vvv, dv), (vvv, dv), (vvv, dv),
                    (vv, dvv), (vv, dvv), (vv, dvv), (vv, dvv), (vv, dvv), (vv, dvv),
                    (v, dvvv), (v, dvvv), (v, dvvv), (v, dvvv),
                ])?,
            ])
        })())
    }
}

/// Actual degree-4 base and four derivative-row writes, then active pole walks.
pub(super) fn fourth_evaluation_cost([u_degree, v_degree]: [usize; 2]) -> Option<usize> {
    let basis_work = |degree: usize| {
        if degree < 4 { return Some(0); }
        let base = degree - 4;
        let base_work = if base <= 1 { 0 } else {
            base.checked_add(2)?
                .checked_add(base.checked_mul(base.checked_add(1)?)?.checked_div(2)?)?
                .checked_add(base)?
        };
        base_work.checked_add(degree.checked_mul(4)?.checked_sub(2)?)
    };
    let supports = u_degree.checked_add(1)?.checked_mul(v_degree.checked_add(1)?)?;
    let sums = usize::from(u_degree >= 4)
        + usize::from(u_degree >= 3 && v_degree >= 1)
        + usize::from(u_degree >= 2 && v_degree >= 2)
        + usize::from(u_degree >= 1 && v_degree >= 3)
        + usize::from(v_degree >= 4);
    let visits = if supports > 2 { supports.checked_mul(sums)? } else { 0 };
    basis_work(u_degree)?.checked_add(basis_work(v_degree)?)?.checked_add(visits)
}

#[cfg(test)]
pub(in crate::eval) mod tests;
