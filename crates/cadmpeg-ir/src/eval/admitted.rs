// SPDX-License-Identifier: Apache-2.0
//! Geometry evaluation with caller-owned decode resource admission.

use std::cell::RefCell;

use cadmpeg_core::decode::{alloc_filled, DecodeContext, DepthGuard, u64_from_index};
use cadmpeg_core::CodecError;

use super::{EvaluationFailure, CurveDerivative};
use crate::features::{FinitePoint3, FiniteVector3};
use crate::geometry::{CurveGeometry, SurfaceGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::pcurve::PcurveGeometry;
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
    context: Option<&'ctx DecodeContext<'arena>>,
    refusal: RefCell<Option<CodecError>>,
}

impl Default for Scratch<'_, '_> {
    fn default() -> Self {
        Self { context: None, refusal: RefCell::new(None) }
    }
}

impl<'ctx, 'arena> Scratch<'ctx, 'arena> {
    fn new(context: &'ctx DecodeContext<'arena>) -> Self {
        Self { context: Some(context), refusal: RefCell::new(None) }
    }

    fn admit<T>(&self, result: Result<T, CodecError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) => {
                let mut refusal = self.refusal.borrow_mut();
                if refusal.is_none() { *refusal = Some(error); }
                None
            }
        }
    }

    pub(super) fn filled<T: Clone>(&self, count: usize, value: T, operation: &'static str) -> Option<Vec<T>> {
        if self.refusal.borrow().is_some() { return None; }
        match self.context {
            Some(ctx) => self.admit(ctx.alloc_filled(count, value, operation)),
            None => alloc_filled(count, value, operation).ok(),
        }
    }

    pub(super) fn collect<T>(&self, values: impl IntoIterator<Item = Option<T>>, operation: &'static str) -> Option<Vec<T>> {
        if self.refusal.borrow().is_some() { return None; }
        let mut output = Vec::new();
        for value in values {
            let value = value?;
            match self.context {
                Some(ctx) => self.admit(ctx.try_reserve_items(&mut output, 1, operation))?,
                None => output.try_reserve(1).ok()?,
            }
            output.push(value);
        }
        Some(output)
    }

    pub(super) fn work(&self, count: usize, operation: &'static str) -> Option<()> {
        if self.refusal.borrow().is_some() { return None; }
        match self.context {
            Some(ctx) => self.admit(ctx.charge_work(u64_from_index(count), operation)),
            None => Some(()),
        }
    }

    pub(super) fn enter(&self) -> Option<Option<DepthGuard<'_>>> {
        if self.refusal.borrow().is_some() { return None; }
        match self.context {
            Some(ctx) => self.admit(ctx.enter_nested("geometry evaluation nesting")).map(Some),
            None => Some(None),
        }
    }

    fn finish<T>(self, value: T) -> Result<T, CodecError> {
        match self.refusal.into_inner() {
            Some(error) => Err(error),
            None => Ok(value),
        }
    }
}

/// Evaluate a stored curve, admitting every scratch allocation and recursive step.
pub fn curve_point(ctx: &DecodeContext<'_>, geometry: &CurveGeometry, parameter: f64)
    -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, CodecError>
{
    let scratch = Scratch::new(ctx);
    let result = geometry.solved().ok_or(EvaluationFailure::NoValue)
        .and_then(|geometry| curve_point_solved(&scratch, geometry, parameter));
    scratch.finish(result)
}

fn curve_point_solved(scratch: &Scratch<'_, '_>, geometry: &SolvedCurveGeometry, parameter: f64)
    -> Result<FinitePoint3, EvaluationFailure<Point3>>
{
    let _depth = scratch.enter().ok_or(EvaluationFailure::NoValue)?;
    match geometry {
        SolvedCurveGeometry::Nurbs(nurbs) => {
            let parameter = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue)?;
            let parameter = super::map_nurbs_curve_parameter(nurbs, parameter).ok_or(EvaluationFailure::NoValue)?;
            let poles = nurbs.pole_rows();
            super::nurbs_curve_point_evaluation(scratch, nurbs.degree(), nurbs.knots(), poles.count(),
                |index| poles.point_at(index), |index| poles.weight_at(index), parameter)
        }
        SolvedCurveGeometry::Transformed(placed) => super::placed_point(*placed.transform(),
            curve_point_solved(scratch, placed.basis(), parameter)),
        _ => super::curve_point_solved(geometry, parameter),
    }
}

/// Evaluate a NURBS curve in its knot domain with caller scratch admission.
pub fn nurbs_curve_point_at(ctx: &DecodeContext<'_>, curve: &NurbsCurve, parameter: f64)
    -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, CodecError>
{
    let scratch = Scratch::new(ctx);
    let poles = curve.pole_rows();
    let result = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue).and_then(|parameter|
        super::nurbs_curve_point_evaluation(&scratch, curve.degree(), curve.knots(), poles.count(),
            |index| poles.point_at(index), |index| poles.weight_at(index), parameter));
    scratch.finish(result)
}

/// Evaluate a stored curve tangent with caller scratch admission.
pub fn curve_tangent(ctx: &DecodeContext<'_>, geometry: &CurveGeometry, parameter: f64)
    -> Result<Result<FiniteVector3, EvaluationFailure<()>>, CodecError>
{
    let scratch = Scratch::new(ctx);
    let result = geometry.solved().ok_or(EvaluationFailure::NoValue).and_then(|geometry|
        super::curve_derivative_evaluation(&scratch, geometry, parameter, CurveDerivative::First));
    scratch.finish(result)
}

/// Evaluate a stored surface point with caller scratch admission.
pub fn surface_point(ctx: &DecodeContext<'_>, geometry: &SurfaceGeometry, u: f64, v: f64)
    -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, CodecError>
{
    let scratch = Scratch::new(ctx);
    let result = geometry.solved().ok_or(EvaluationFailure::NoValue)
        .and_then(|geometry| surface_point_solved(&scratch, geometry, u, v));
    scratch.finish(result)
}

fn surface_point_solved(scratch: &Scratch<'_, '_>, geometry: &SolvedSurfaceGeometry, u: f64, v: f64)
    -> Result<FinitePoint3, EvaluationFailure<Point3>>
{
    let _depth = scratch.enter().ok_or(EvaluationFailure::NoValue)?;
    match geometry {
        SolvedSurfaceGeometry::Nurbs(nurbs) => super::nurbs_surface_local(scratch, nurbs, u, v)
            .map(|local| { let [x, y, z] = local.point; FinitePoint3::from_coordinates(x, y, z) }),
        SolvedSurfaceGeometry::Transformed(placed) => super::placed_point(*placed.transform(),
            surface_point_solved(scratch, placed.basis(), u, v)),
        _ => super::surface_point_solved(geometry, u, v),
    }
}

/// Evaluate a stored pcurve point with caller scratch admission.
pub fn pcurve_uv(ctx: &DecodeContext<'_>, geometry: &PcurveGeometry, parameter: f64)
    -> Result<Result<FinitePoint2, EvaluationFailure<Point2>>, CodecError>
{
    let scratch = Scratch::new(ctx);
    let result = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue).and_then(|parameter|
        super::pcurve_uv_differential(&scratch, geometry, parameter)
            .ok_or(EvaluationFailure::NoValue)?.point.map_err(EvaluationFailure::NonFinite));
    scratch.finish(result)
}

/// Reusable point-evaluation storage for repeated parameters on one NURBS curve.
/// The basis is admitted once; evaluations mutate that storage and borrow poles.
pub struct NurbsPointEvaluator<'curve> {
    curve: &'curve NurbsCurve,
    basis: Vec<f64>,
}

impl<'curve> NurbsPointEvaluator<'curve> {
    /// Admit the basis storage before allocating it.
    pub fn new(ctx: &DecodeContext<'_>, curve: &'curve NurbsCurve) -> Result<Self, CodecError> {
        let support = curve.knots().len() - curve.pole_count();
        let basis = ctx.alloc_filled(support, 0.0, "IR B-spline basis")?;
        Ok(Self { curve, basis })
    }

    /// Evaluate in the knot domain without allocating another basis or pole window.
    pub fn point(&mut self, ctx: &DecodeContext<'_>, parameter: f64)
        -> Result<Result<FinitePoint3, EvaluationFailure<Point3>>, CodecError>
    {
        let _depth = ctx.enter_nested("geometry evaluation nesting")?;
        let degree = self.basis.len() - 1;
        for _ in 0..self.basis.len() {
            ctx.charge_work(u64_from_index(self.basis.len()), "IR B-spline basis work")?;
        }
        let result = (|| {
            let parameter = FiniteReal::new(parameter).ok_or(EvaluationFailure::NoValue)?;
            let span = super::bspline_span(self.curve.knots(), degree, self.curve.pole_count(), parameter.get())
                .ok_or(EvaluationFailure::NoValue)?;
            let unreached = EvaluationFailure::NonFinite(super::UNREACHED_POINT);
            super::bspline_basis_into(self.curve.knots(), degree, span, parameter.get(), &mut self.basis)
                .ok_or(unreached)?;
            if !self.basis.iter().all(|value| value.is_finite()) { return Err(unreached); }
            let poles = self.curve.pole_rows();
            let base = super::Homogeneous::sum(self.basis.iter().copied().enumerate().map(|(local, basis)| {
                let index = span - degree + local;
                Some(([basis, 1.0], poles.weight_at(index).unwrap_or(1.0), poles.point_at(index)?))
            })).ok_or(EvaluationFailure::NoValue)?;
            let [x, y, z] = super::finite_lanes(base.project(base, &[]).ok_or(EvaluationFailure::NoValue)?)
                .map_err(|[x, y, z]| EvaluationFailure::NonFinite(Point3::new(x, y, z)))?;
            Ok(FinitePoint3::from_coordinates(x, y, z))
        })();
        Ok(result)
    }
}

#[cfg(test)]
mod tests;
