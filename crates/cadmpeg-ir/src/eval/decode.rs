// SPDX-License-Identifier: Apache-2.0
//! Geometry evaluation with caller-owned decode resource admission.

use std::cell::RefCell;

use cadmpeg_core::decode::{
    u64_from_index, DecodeContext, DepthGuard, ResourceLimit, ScopedReservation,
};

use super::{CurveDerivative, EvaluationFailure};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::pcurve::PcurveGeometry;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use crate::math::{Point2, Point3};
use crate::scalar::FiniteReal;
use crate::units::FinitePoint2;

/// A basis or pole window with fixed storage for constant and linear spans.
pub(super) enum SupportValues<T> {
    Inline { values: [T; 2], len: usize },
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

/// Scratch admission shared by one evaluation and its recursive calls.
pub(super) struct Scratch<'ctx, 'arena> {
    pub(super) context: &'ctx DecodeContext<'arena>,
    storage: RefCell<Option<ScopedReservation<'ctx>>>,
    refusal: RefCell<Option<ResourceLimit>>,
}

impl<'ctx, 'arena> Scratch<'ctx, 'arena> {
    pub(super) fn new(context: &'ctx DecodeContext<'arena>) -> Self {
        Self {
            context,
            storage: RefCell::new(None),
            refusal: RefCell::new(None),
        }
    }

    pub(super) fn admit<T>(&self, result: Result<T, ResourceLimit>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                let mut refusal = self.refusal.borrow_mut();
                if refusal.is_none() {
                    *refusal = Some(error);
                }
                None
            }
        }
    }

    /// The resource refusal recorded by an allocation, charge or evaluation.
    pub(super) fn refused(&self) -> Option<ResourceLimit> {
        *self.refusal.borrow()
    }

    /// `Err` with the recorded resource refusal, or `Ok` when nothing was refused.
    pub(super) fn unless_refused(&self) -> Result<(), ResourceLimit> {
        self.refused().map_or(Ok(()), Err)
    }

    /// The recorded resource refusal, or `absent` when a value is missing for
    /// another reason.
    pub(super) fn failure<R>(&self, absent: EvaluationFailure<R>) -> EvaluationFailure<R> {
        self.refused()
            .map_or(absent, EvaluationFailure::ResourceLimit)
    }

    /// The result, or the recorded resource refusal when one was recorded.
    pub(super) fn settle<T, R>(
        &self,
        result: Result<T, EvaluationFailure<R>>,
    ) -> Result<T, EvaluationFailure<R>> {
        self.refused()
            .map_or(result, |limit| Err(EvaluationFailure::ResourceLimit(limit)))
    }

    pub(super) fn filled<T: Clone>(
        &self,
        count: usize,
        value: T,
        operation: &'static str,
        work_operation: &'static str,
    ) -> Option<Vec<T>> {
        if self.refusal.borrow().is_some() {
            return None;
        }
        let mut values = Vec::new();
        self.reserve(&mut values, count, operation)?;
        for _ in 0..count {
            self.work(1, work_operation)?;
            values.push(value.clone());
        }
        Some(values)
    }

    pub(super) fn collect<T>(
        &self,
        values: impl IntoIterator<Item = Option<T>>,
        operation: &'static str,
        work_operation: &'static str,
    ) -> Option<Vec<T>> {
        if self.refusal.borrow().is_some() {
            return None;
        }
        self.work(0, work_operation)?;
        let mut values = values.into_iter();
        let mut output = Vec::new();
        while values.size_hint() != (0, Some(0)) {
            self.work(1, work_operation)?;
            let Some(value) = values.next() else { break; };
            self.reserve(&mut output, 1, operation)?;
            output.push(value?);
        }
        Some(output)
    }

    pub(super) fn reserve<T>(&self, values: &mut Vec<T>, count: usize, operation: &'static str) -> Option<()> {
        let mut storage = self.storage.borrow_mut();
        if storage.is_none() {
            *storage = Some(self.admit(self.context.reserve_scoped_limit(0, operation))?);
        }
        let reservation = storage.as_mut()?;
        self.admit(
            self.context
                .reserve_scoped_vec_limit(reservation, values, count, operation),
        )
    }

    pub(super) fn work(&self, count: usize, operation: &'static str) -> Option<()> {
        if self.refusal.borrow().is_some() {
            return None;
        }
        self.admit(
            self.context
                .charge_work_limit(u64_from_index(count), operation),
        )
    }

    pub(super) fn enter(&self) -> Option<DepthGuard<'_>> {
        if self.refusal.borrow().is_some() {
            return None;
        }
        self.admit(
            self.context
                .enter_nested_limit("geometry evaluation nesting"),
        )
    }

    pub(super) fn finish<T>(self, value: T) -> Result<T, ResourceLimit> {
        match self.refusal.into_inner() {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }

    /// Finishes an evaluation: a recorded refusal, or a resource refusal the
    /// evaluation reports, is the outer error.
    pub(super) fn finish_evaluation<T, R>(
        self,
        result: Result<T, EvaluationFailure<R>>,
    ) -> Result<Result<T, EvaluationFailure<R>>, ResourceLimit> {
        outer_refusal(self.finish(result)?)
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
fn outer_refusal<T, R>(
    result: Result<T, EvaluationFailure<R>>,
) -> Result<Result<T, EvaluationFailure<R>>, ResourceLimit> {
    match result {
        Err(EvaluationFailure::ResourceLimit(limit)) => Err(limit),
        result => Ok(result),
    }
}

/// Evaluate a stored curve, admitting every scratch allocation and recursive step.
pub fn curve_point_for_decode(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
    parameter: f64,
) -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
    let result = geometry
        .solved()
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|geometry| super::curve_point_evaluation(&scratch, geometry, parameter));
    scratch.finish_evaluation(result)
}

/// Evaluate a NURBS curve in its knot domain with caller scratch admission.
pub fn nurbs_curve_point_at_for_decode(
    ctx: &DecodeContext<'_>,
    curve: &NurbsCurve,
    parameter: f64,
) -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
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
    scratch.finish_evaluation(result)
}

/// Evaluate a stored curve tangent with caller scratch admission.
pub fn curve_tangent_for_decode(
    ctx: &DecodeContext<'_>,
    geometry: &CurveGeometry,
    parameter: f64,
) -> Result<Result<FiniteVector3, EvaluationFailure<()>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
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
    scratch.finish_evaluation(result)
}

/// Evaluate a stored surface point with caller scratch admission.
pub fn surface_point_for_decode(
    ctx: &DecodeContext<'_>,
    geometry: &SurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
    let result = geometry
        .solved()
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|geometry| super::surface_point_evaluation(&scratch, geometry, u, v));
    scratch.finish_evaluation(result)
}

/// Evaluate a stored pcurve point with caller scratch admission.
pub fn pcurve_uv_for_decode(
    ctx: &DecodeContext<'_>,
    geometry: &PcurveGeometry,
    parameter: f64,
) -> Result<Result<FinitePoint2, EvaluationFailure<Point2>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
    let result = FiniteReal::new(parameter)
        .ok_or(EvaluationFailure::NoValue)
        .and_then(|parameter| {
            let evaluated = super::pcurve_uv_differential(&scratch, geometry, parameter)
                .ok_or(EvaluationFailure::NoValue)?;
            if let Some(limit) = evaluated.resource {
                return Err(EvaluationFailure::ResourceLimit(limit));
            }
            evaluated.point.map_err(EvaluationFailure::NonFinite)
        });
    scratch.finish_evaluation(result)
}

/// Reusable point-evaluation storage for repeated parameters on one NURBS curve.
/// The basis is admitted once; evaluations mutate that storage and borrow poles.
pub struct NurbsPointEvaluator<'curve, 'ctx> {
    curve: &'curve NurbsCurve,
    basis: SupportValues<f64>,
    _storage: Option<ScopedReservation<'ctx>>,
}

impl<'curve, 'ctx> NurbsPointEvaluator<'curve, 'ctx> {
    /// Admit the basis storage before allocating it.
    pub fn new(
        ctx: &'ctx DecodeContext<'_>,
        curve: &'curve NurbsCurve,
    ) -> Result<Self, ResourceLimit> {
        let support = curve.knots().len() - curve.pole_count();
        let (basis, storage) = if support <= 2 {
            (
                SupportValues::Inline {
                    values: [0.0; 2],
                    len: support,
                },
                None,
            )
        } else {
            {
                let mut basis = Vec::new();
                let storage =
                    ctx.reserve_temporary_vec(&mut basis, support, "IR B-spline basis")?;
                ctx.charge_work_limit(u64_from_index(support), "IR B-spline basis work")?;
                basis.extend(std::iter::repeat_n(0.0, support));
                (SupportValues::Heap(basis), Some(storage))
            }
        };
        Ok(Self {
            curve,
            basis,
            _storage: storage,
        })
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
            ).map_err(EvaluationFailure::ResourceLimit)?
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
            .map_err(EvaluationFailure::ResourceLimit)?
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

/// Evaluate a borrowed solved curve with caller-owned scratch admission.
pub fn curve_point_solved_for_decode(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedCurveGeometry,
    parameter: f64,
) -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
    let result = super::curve_point_evaluation(&scratch, geometry, parameter);
    scratch.finish_evaluation(result)
}

/// Evaluate a borrowed solved surface with caller-owned scratch admission.
pub fn surface_point_solved_for_decode(
    ctx: &DecodeContext<'_>,
    geometry: &SolvedSurfaceGeometry,
    u: f64,
    v: f64,
) -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
    let result = super::surface_point_evaluation(&scratch, geometry, u, v);
    scratch.finish_evaluation(result)
}

/// Evaluate a NURBS surface with scoped caller storage.
pub fn nurbs_surface_point_for_decode(
    ctx: &DecodeContext<'_>,
    surface: &crate::geometry::nurbs::NurbsSurface,
    u: f64,
    v: f64,
) -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, ResourceLimit> {
    let scratch = Scratch::new(ctx);
    let result = super::nurbs_surface_local(&scratch, surface, u, v).map(|local| {
        let [point_x, point_y, point_z] = local.point;
        FinitePoint3::from_coordinates(point_x, point_y, point_z)
    });
    scratch.finish_evaluation(result)
}
