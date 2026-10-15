// SPDX-License-Identifier: Apache-2.0
//! Geometry evaluation with caller-owned decode resource admission.

use std::cell::Cell;

use cadmpeg_core::decode::{DecodeContext, ResourceLimit};

use super::admission::{EvaluationAdmission, EvaluationDepthGuard, EvaluationRefusal};
use super::{CurveDerivative, EvaluationFailure};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::pcurve::PcurveGeometry;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point2, Point3};
use crate::scalar::FiniteReal;
use crate::units::FinitePoint2;

/// Inline spline support through degree 15; larger windows use admitted backing.
pub(super) const INLINE_SUPPORT: usize = 16;

/// A basis or pole window with bounded inline storage.
#[derive(Debug, PartialEq)]
pub(super) enum SupportValues<T> {
    Inline {
        values: [T; INLINE_SUPPORT],
        len: usize,
    },
    Heap(Vec<T>),
}

impl<T> std::ops::Deref for SupportValues<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        match self {
            Self::Inline { values, len } => &values[..*len],
            Self::Heap(values) => values,
        }
    }
}

impl<T> std::ops::DerefMut for SupportValues<T> {
    fn deref_mut(&mut self) -> &mut [T] {
        match self {
            Self::Inline { values, len } => &mut values[..*len],
            Self::Heap(values) => values,
        }
    }
}

/// Refusal state shared by one evaluation and its recursive calls.
pub(super) struct Scratch<'ctx, 'arena> {
    pub(super) admission: EvaluationAdmission<'ctx, 'arena>,
    independent_depth: Cell<usize>,
    refusal: Cell<Option<EvaluationRefusal>>,
}

impl<'ctx, 'arena> Scratch<'ctx, 'arena> {
    pub(super) fn new(admission: impl Into<EvaluationAdmission<'ctx, 'arena>>) -> Self {
        Self {
            admission: admission.into(),
            independent_depth: Cell::new(0),
            refusal: Cell::new(None),
        }
    }

    pub(super) fn admit<T>(&self, result: Result<T, impl Into<EvaluationRefusal>>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                if !matches!(
                    self.refusal.get(),
                    Some(EvaluationRefusal::ResourceLimit(_))
                ) {
                    self.refusal.set(Some(error.into()));
                }
                None
            }
        }
    }

    /// The refusal recorded by a step or held by the admission. A session
    /// refusal takes precedence over an exhausted work slice.
    pub(super) fn refused(&self) -> Option<EvaluationRefusal> {
        let recorded = self.refusal.get();
        if let Some(EvaluationRefusal::ResourceLimit(limit)) = recorded {
            return Some(EvaluationRefusal::ResourceLimit(limit));
        }
        let refusal = self.admission.refusal().or(recorded);
        self.refusal.set(refusal);
        refusal
    }

    /// `Err` with the recorded refusal, or `Ok` when nothing was refused.
    pub(super) fn unless_refused(&self) -> Result<(), EvaluationRefusal> {
        self.refused().map_or(Ok(()), Err)
    }

    /// The recorded refusal, or `absent` when a value is missing for another
    /// reason.
    pub(super) fn failure<R>(&self, absent: EvaluationFailure<R>) -> EvaluationFailure<R> {
        self.refused().map_or(absent, EvaluationFailure::from)
    }

    /// The result, or the recorded refusal when one was recorded.
    pub(super) fn settle<T, R>(
        &self,
        result: Result<T, EvaluationFailure<R>>,
    ) -> Result<T, EvaluationFailure<R>> {
        self.refused()
            .map_or(result, |refusal| Err(EvaluationFailure::from(refusal)))
    }

    /// `count` copies of `value`, one work step each.
    pub(super) fn filled<T: Clone>(&self, count: usize, value: T) -> Option<Vec<T>> {
        self.work(count)?;
        Some((0..count).map(|_| value.clone()).collect())
    }

    /// Collect present values, one work step per item drawn.
    pub(super) fn collect<T>(&self, values: impl IntoIterator<Item = Option<T>>) -> Option<Vec<T>> {
        self.work(0)?;
        let mut values = values.into_iter();
        let mut output = Vec::new();
        while values.size_hint() != (0, Some(0)) {
            self.work(1)?;
            let Some(value) = values.next() else {
                break;
            };
            output.push(value?);
        }
        Some(output)
    }

    /// Collect a local spline window without heap backing through degree 15.
    pub(super) fn support_values<T: Copy + Default>(
        &self,
        values: impl ExactSizeIterator<Item = Option<T>>,
    ) -> Option<SupportValues<T>> {
        let len = values.len();
        if len > INLINE_SUPPORT {
            return self.collect(values).map(SupportValues::Heap);
        }
        self.work(0)?;
        let mut output = SupportValues::Inline {
            values: [T::default(); INLINE_SUPPORT],
            len,
        };
        for (index, value) in values.enumerate() {
            self.work(1)?;
            output[index] = value?;
        }
        Some(output)
    }

    /// Copy `source`, one work step per copied byte.
    pub(super) fn copied<T: Copy>(&self, source: &[T]) -> Option<Vec<T>> {
        self.work(std::mem::size_of_val(source))?;
        Some(source.to_vec())
    }

    pub(super) fn work(&self, count: usize) -> Option<()> {
        if self.refusal.get().is_some() {
            return None;
        }
        self.admit(self.admission.work(count))
    }

    pub(super) fn enter(&self) -> Option<EvaluationDepthGuard<'_>> {
        if self.refusal.get().is_some() {
            return None;
        }
        self.admit(self.admission.enter(&self.independent_depth))
    }

    /// Finishes an evaluation: a recorded resource refusal, or one the
    /// evaluation reports, is the outer error. An exhausted work slice stays
    /// the inner [`EvaluationFailure::WorkExhausted`].
    pub(super) fn finish_evaluation<T, R>(
        self,
        result: Result<T, EvaluationFailure<R>>,
    ) -> Result<Result<T, EvaluationFailure<R>>, ResourceLimit> {
        outer_refusal(self.settle(result))
    }

    /// The optional result, or the recorded refusal when one was recorded: a
    /// session refusal is the error and an exhausted work slice is no value.
    pub(super) fn settle_option<T, E: From<ResourceLimit>>(
        &self,
        result: Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        match self.refused() {
            None => result,
            Some(refusal) => Ok(refusal.absent()?),
        }
    }
}

impl<R> From<cadmpeg_core::CodecError> for EvaluationFailure<R> {
    fn from(error: cadmpeg_core::CodecError) -> Self {
        match error {
            cadmpeg_core::CodecError::ResourceLimit(limit) => Self::ResourceLimit(limit),
            _ => Self::NoValue,
        }
    }
}

/// Moves a resource refusal reported by an evaluation to the outer error.
pub fn outer_refusal<T, R>(
    result: Result<T, EvaluationFailure<R>>,
) -> Result<Result<T, EvaluationFailure<R>>, ResourceLimit> {
    match result {
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit),
        result => Ok(result),
    }
}

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization.
/// A procedural carrier without a solved cache has no value here.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn curve_point<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    geometry: &CurveGeometry,
    parameter: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let scratch = Scratch::new(admission);
    let result = geometry
        .solved()
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|geometry| super::curve_point_evaluation(&scratch, geometry, parameter));
    scratch.settle(result)
}

/// Evaluate a NURBS curve at knot-domain parameter `t` over its admitted
/// poles, or report why it has no finite point there.
///
/// A parameter that is not finite, a knot vector that states no span at `t`,
/// and a zero weight sum have no value. A basis that leaves the finite range
/// reaches no coordinate, and each reads NaN; a projection that overflows
/// carries each coordinate it reached.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn nurbs_curve_point_at<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    curve: &NurbsCurve,
    parameter: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let scratch = Scratch::new(admission);
    let poles = curve.pole_rows();
    let result = FiniteReal::new(parameter)
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|parameter| {
            super::nurbs_curve_point_evaluation(
                &scratch,
                curve.degree(),
                curve.knots(),
                poles.count(),
                |index| poles.point_at(index),
                |index| poles.weight_at(index),
                parameter,
            )
        });
    scratch.settle(result)
}

/// Evaluate the exact first derivative of a stored curve carrier, or report
/// why it has no finite value, as [`super::curve_tangent_solved`] states. A
/// procedural carrier without a solved cache has no value here.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn curve_tangent<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    geometry: &CurveGeometry,
    parameter: f64,
) -> Result<FiniteVector3, EvaluationFailure<()>> {
    let scratch = Scratch::new(admission);
    let result = geometry
        .solved()
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|geometry| {
            super::curve_derivative_evaluation(
                &scratch,
                geometry,
                parameter,
                CurveDerivative::First,
            )
        });
    scratch.settle(result)
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn surface_point<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let scratch = Scratch::new(admission);
    let result = geometry
        .solved()
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|geometry| super::surface_point_evaluation(&scratch, geometry, u, v));
    scratch.settle(result)
}

/// Evaluate a pcurve carrier at parameter `t`, yielding a surface `(u, v)`.
///
/// Evaluation is total over the carrier's stated shape and does not consult a
/// declared domain. A NURBS carrier extrapolates its end span past the knot
/// interval, a trim hands `t` to its basis outside the trim interval, and a
/// line and a conic evaluate at every finite `t`. The failure is never that
/// `t` is out of domain.
///
/// An evaluation that leaves the finite range reports
/// [`EvaluationFailure::NonFinite`] with the point it reached; a coordinate
/// that no step reached is NaN. A carrier that evaluates its derivatives
/// together with its point also reports its point this way when a derivative
/// leaves the finite range. A parameter that is not finite, a structure that
/// states no point at `t`, and an undefined step (a polar chart at its
/// origin, an offset whose basis tangent is zero) report
/// [`EvaluationFailure::NoValue`].
///
/// Callers that recover a parameter from an unreliable declared interval
/// depend on this: they seed and step outside the interval and use the
/// evaluated point as the witness. A caller that wants the domain asks
/// the carrier for it.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn pcurve_uv<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    geometry: &PcurveGeometry,
    parameter: f64,
) -> Result<FinitePoint2, EvaluationFailure<Point2>> {
    let scratch = Scratch::new(admission);
    let result = FiniteReal::new(parameter)
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|parameter| {
            let evaluated = super::pcurve_uv_differential(&scratch, geometry, parameter)
                .ok_or(EvaluationFailure::NoValue)?;
            if let Some(refusal) = evaluated.refusal {
                return Err(refusal.into());
            }
            evaluated.point.map_err(EvaluationFailure::NonFinite)
        });
    scratch.settle(result)
}

/// Reusable point-evaluation storage for repeated parameters on one NURBS curve.
/// The basis is allocated once; evaluations mutate that storage and borrow poles.
pub struct NurbsPointEvaluator<'curve> {
    curve: &'curve NurbsCurve,
    basis: SupportValues<f64>,
}

impl<'curve> NurbsPointEvaluator<'curve> {
    /// Allocate the basis storage for `curve`.
    pub fn new(curve: &'curve NurbsCurve) -> Self {
        let support = curve.knots().len() - curve.pole_count();
        let basis = if support <= INLINE_SUPPORT {
            SupportValues::Inline {
                values: [0.0; INLINE_SUPPORT],
                len: support,
            }
        } else {
            SupportValues::Heap((0..support).map(|_| 0.0).collect())
        };
        Self { curve, basis }
    }

    /// Evaluate in the knot domain without allocating another basis or pole window.
    pub fn point(
        &mut self,
        ctx: &DecodeContext<'_>,
        parameter: f64,
    ) -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, ResourceLimit> {
        let _depth = if matches!(&self.basis, SupportValues::Heap(_)) {
            let depth = ctx.enter_nested_limit("geometry evaluation nesting")?;
            Some(depth)
        } else {
            None
        };
        let degree = self.basis.len() - 1;
        let result = (|| {
            let parameter = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue)?;
            let span = super::basis::bspline_span(
                ctx,
                self.curve.knots(),
                degree,
                self.curve.pole_count(),
                parameter.get(),
            )
            .map_err(EvaluationFailure::from)?
            .ok_or(EvaluationFailure::NoValue)?;
            let unreached = EvaluationFailure::NonFinite(super::UNREACHED_POINT);
            super::basis::fill_bspline_basis(
                ctx,
                self.curve.knots(),
                degree,
                span,
                parameter.get(),
                &mut self.basis,
            )
            .map_err(EvaluationFailure::from)?
            .ok_or(unreached)?;
            let poles = self.curve.pole_rows();
            let scratch = Scratch::new(ctx);
            scratch.settle(super::nurbs_curve_point_from_basis(
                &scratch,
                &self.basis,
                span,
                |index| poles.point_at(index),
                |index| poles.weight_at(index),
            ))
        })();
        outer_refusal(result)
    }
}

#[cfg(test)]
mod tests;

/// Evaluate a 3D curve carrier at parameter `t` on its own parameterization,
/// or report why it has no finite point there.
///
/// A parameter that is not finite has no value on every carrier that reads
/// it; a degenerate carrier is its point at every parameter. A point outside
/// the finite range is non-finite and carries the point the arm reached; a
/// coefficient outside the finite range reaches no coordinate, and each reads
/// NaN.
///
/// The descent is bounded by [`PlacedCurve`](crate::geometry::PlacedCurve)
/// construction; no arm follows an arena id.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn curve_point_solved<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SolvedCurveGeometry,
    parameter: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let scratch = Scratch::new(admission);
    let result = super::curve_point_evaluation(&scratch, geometry, parameter);
    scratch.settle(result)
}

/// Evaluate a surface carrier at `(u, v)` on its own parameterization: `u` is
/// the azimuth angle and `v` the axial distance / polar angle on analytic
/// quadrics, and both are knot-domain parameters on NURBS surfaces.
///
/// The evaluation fails only on the point: every carrier evaluates its point
/// alone, so partials outside the finite range at a finite point leave the
/// point finite.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn surface_point_solved<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let scratch = Scratch::new(admission);
    let result = super::surface_point_evaluation(&scratch, geometry, u, v);
    scratch.settle(result)
}

/// Evaluate a tensor-product NURBS surface at `(u, v)`, or report why it has
/// no finite point there.
///
/// A parameter that is not finite, a knot vector or pole net that states no
/// span at the parameter, and a zero weight sum have no value. A basis that
/// leaves the finite range reaches no coordinate, and each reads NaN; a
/// projection that overflows carries each coordinate it reached.
///
/// The admission policy supplies session accounting or standard-library storage.
pub fn nurbs_surface_point<'ctx, 'arena: 'ctx>(
    admission: impl Into<EvaluationAdmission<'ctx, 'arena>>,
    surface: &crate::geometry::nurbs::NurbsSurface,
    u: f64,
    v: f64,
) -> Result<FinitePoint3, EvaluationFailure<Point3>> {
    let scratch = Scratch::new(admission);
    let result = scratch
        .admission
        .independent_cost(super::nurbs_surface_evaluation_cost(surface))
        .and_then(|()| super::nurbs_surface_local(&scratch, surface, u, v))
        .map(|local| {
            let [point_x, point_y, point_z] = local.point;
            FinitePoint3::from_coordinates(point_x, point_y, point_z)
        });
    scratch.settle(result)
}
